//! Client for interacting with the JavaScript executor service
//! (crates/js_executor). This client can either connect to an existing
//! js_executor service via a Unix socket, or it can spawn and manage its own
//! js_executor process.

use std::{
    env,
    future::Future,
    path::PathBuf,
    pin::Pin,
    process::Stdio,
    task::{Context, Poll},
    time::Duration,
};

use common_models::{ErrorResponse, ExecutionRequest, ExecutionResponse};
use http_body_util::Full;
use hyper::{Uri, body::Bytes};
use hyper_util::{client::legacy::Client, rt::TokioIo};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    net::UnixStream,
    process::{Child, Command},
};
use tower::Service;

use crate::models::monitor_match::MonitorMatch;

/// Unix socket connector for hyper
#[derive(Clone)]
struct UnixConnector {
    socket_path: PathBuf,
}

impl Service<Uri> for UnixConnector {
    type Error = std::io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;
    type Response = TokioIo<UnixStream>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: Uri) -> Self::Future {
        let socket_path = self.socket_path.clone();
        Box::pin(async move {
            let stream = UnixStream::connect(socket_path).await?;
            Ok(TokioIo::new(stream))
        })
    }
}

const REQUEST_TIMEOUT_SECONDS: u64 = 5;

/// Trait defining the interface for a JavaScript executor client.
#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait JsClient: Send + Sync {
    /// Submits a JavaScript script to the executor for execution.
    async fn submit_script(
        &self,
        script: String,
        context: &MonitorMatch,
    ) -> Result<ExecutionResponse, JsExecutorClientError>;
}

/// Client for interacting with the JavaScript executor service.
pub struct JsExecutorClient {
    /// The child process running the JavaScript executor.
    executor_child: Option<Child>,
    /// Hyper client for making requests to the JavaScript executor via Unix
    /// socket.
    hyper_client: Client<UnixConnector, Full<Bytes>>,
}

/// Errors that can occur when interacting with the JavaScript executor
#[derive(Debug, thiserror::Error)]
pub enum JsExecutorClientError {
    /// Error spawning the JavaScript executor process.
    #[error("Failed to spawn JavaScript executor: {0}")]
    SpawnError(#[from] std::io::Error),

    /// Error serializing/deserializing JSON data.
    #[error("JSON serialization error: {0}")]
    JsonError(#[from] serde_json::Error),

    /// Error with hyper HTTP operations.
    #[error("HTTP error: {0}")]
    HttpError(#[from] hyper::http::Error),

    /// Error with hyper client operations.
    #[error("Client error: {0}")]
    ClientError(#[from] hyper_util::client::legacy::Error),

    /// Error with hyper operations.
    #[error("Hyper error: {0}")]
    HyperError(#[from] hyper::Error),

    /// The request to the JavaScript executor timed out.
    #[error("Request to JavaScript executor timed out")]
    RequestTimeout,

    /// The JavaScript executor failed to start up properly.
    #[error("JavaScript executor failed to start up")]
    StartupFailed,

    /// An error was returned by the script executor service.
    #[error("JavaScript execution failed: {0}")]
    ScriptExecutionError(String),
}

/// Finds the command to start the JavaScript executor.
fn find_js_executor_command() -> Command {
    // 1. Explicit environment variable is highest priority
    if let Ok(bin_path) = env::var("JS_EXECUTOR_BIN_PATH") {
        return Command::new(bin_path);
    }

    // 2. Check next to the current executable (common for release builds)
    if let Ok(mut path) = env::current_exe() {
        path.pop();
        path.push("js_executor");
        if path.exists() {
            return Command::new(path);
        }
    }

    // 3. Cargo's environment variable for integration tests
    if let Ok(bin_path) = env::var("CARGO_BIN_EXE_js_executor") {
        return Command::new(bin_path);
    }

    // 4. Fallback for development: `cargo run` from js_executor directory
    let mut cmd = Command::new("cargo");
    cmd.arg("run").arg("--");
    
    // Set working directory to js_executor crate
    if let Ok(mut workspace_root) = env::current_dir() {
        // If we're in a subdirectory, try to find the workspace root
        while !workspace_root.join("Cargo.toml").exists() && workspace_root.parent().is_some() {
            workspace_root = workspace_root.parent().unwrap().to_path_buf();
        }
        let js_executor_dir = workspace_root.join("crates").join("js_executor");
        if js_executor_dir.exists() {
            cmd.current_dir(js_executor_dir);
        }
    }
    
    cmd
}

/// Maximum shift for exponential backoff (50ms << 6 = 3.2 seconds).
const MAX_BACKOFF_SHIFT: u32 = 6;

/// Waits for the Unix socket at `socket_path` to become available.
/// This function attempts to connect to the socket multiple times with
/// exponential backoff. If the socket does not become available within the
/// timeout period, it returns a `StartupFailed` error.
async fn wait_for_socket(socket_path: &PathBuf) -> Result<(), JsExecutorClientError> {
    for attempt in 0..10 {
        if tokio::net::UnixStream::connect(socket_path).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50 << attempt.min(MAX_BACKOFF_SHIFT))).await;
    }
    Err(JsExecutorClientError::StartupFailed)
}

impl JsExecutorClient {
    /// Creates a new instance of the JavaScript executor client.
    pub async fn new() -> Result<Self, JsExecutorClientError> {
        let (socket_path, executor_child) = if let Ok(socket_path) =
            env::var("JS_EXECUTOR_SOCKET_PATH")
        {
            // Existing socket path
            (PathBuf::from(socket_path), None)
        } else {
            // Start a new js_executor process
            let pid = std::process::id();
            let socket_path = env::temp_dir().join(format!("argus-js-executor-{}.sock", pid));

            let mut cmd = find_js_executor_command();
            cmd.arg("--socket-path").arg(&socket_path);

            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());

            let mut child = cmd.spawn()?;

            let stdout = child.stdout.take().expect("Failed to capture stdout");
            let stderr = child.stderr.take().expect("Failed to capture stderr");

            // Spawn a task to read and log stdout and stderr from the child process
            tokio::spawn(async move {
                let mut stdout_reader = BufReader::new(stdout);
                let mut stderr_reader = BufReader::new(stderr);

                let mut stdout_line = String::new();
                let mut stderr_line = String::new();

                let mut stdout_closed = false;
                let mut stderr_closed = false;

                // Loop until both stdout and stderr streams are closed.
                while !stdout_closed || !stderr_closed {
                    tokio::select! {
                        // Read from stdout if the stream is not closed yet.
                        res = stdout_reader.read_line(&mut stdout_line), if !stdout_closed => {
                            match res {
                                Ok(0) | Err(_) => stdout_closed = true, // EOF or error
                                Ok(_) => {
                                    tracing::debug!(target: "js_executor", "{}", stdout_line.trim());
                                    stdout_line.clear();
                                }
                            }
                        },
                        // Read from stderr if the stream is not closed yet.
                        res = stderr_reader.read_line(&mut stderr_line), if !stderr_closed => {
                            match res {
                                Ok(0) | Err(_) => stderr_closed = true, // EOF or error
                                Ok(_) => {
                                    tracing::warn!(target: "js_executor", "{}", stderr_line.trim());
                                    stderr_line.clear();
                                }
                            }
                        },
                        // This branch is taken when both streams are closed.
                        else => break,
                    }
                }
                tracing::debug!("JS executor stdout/stderr streams closed. Logging task finished.");
            });

            (socket_path, Some(child))
        };

        // Build the hyper client
        let connector = UnixConnector { socket_path: socket_path.clone() };
        let hyper_client: Client<UnixConnector, Full<Bytes>> =
            Client::builder(hyper_util::rt::TokioExecutor::new()).build(connector);

        // Wait for the socket to become available
        wait_for_socket(&socket_path).await?;
        tracing::debug!("JavaScript executor socket is ready at {}", socket_path.display());

        Ok(Self { executor_child, hyper_client })
    }
}

#[async_trait::async_trait]
impl JsClient for JsExecutorClient {
    /// Submits a JavaScript script to the executor for execution.
    async fn submit_script(
        &self,
        script: String,
        context: &MonitorMatch,
    ) -> Result<ExecutionResponse, JsExecutorClientError> {
        // Create the request body
        let context = serde_json::to_value(context)?;
        let request = ExecutionRequest { script, context };
        let body = serde_json::to_vec(&request)?;

        // Build the HTTP request
        let req = hyper::Request::builder()
            .method(hyper::Method::POST)
            .uri("http://unix/execute") // The URI is ignored by the Unix socket connector
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(body)))?;

        // Send the request with a timeout
        let response = tokio::time::timeout(
            Duration::from_secs(REQUEST_TIMEOUT_SECONDS),
            self.hyper_client.request(req),
        )
        .await
        .map_err(|_| JsExecutorClientError::RequestTimeout)?
        .map_err(JsExecutorClientError::ClientError)?;

        let status = response.status();
        let body_bytes = http_body_util::BodyExt::collect(response.into_body()).await?.to_bytes();

        if !status.is_success() {
            let err_resp: ErrorResponse = serde_json::from_slice(&body_bytes)?;
            return Err(JsExecutorClientError::ScriptExecutionError(err_resp.error));
        }

        let json: ExecutionResponse = serde_json::from_slice(&body_bytes)?;

        Ok(json)
    }
}

impl Drop for JsExecutorClient {
    fn drop(&mut self) {
        tracing::debug!("Dropping JsExecutorClient");
        if let Some(mut child) = self.executor_child.take() {
            tokio::spawn(async move {
                if let Err(e) = child.kill().await {
                    tracing::error!("Failed to kill JavaScript executor process: {}", e);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use axum::{Json, Router, routing::post};
    use hyper::StatusCode;
    use tokio::net::UnixListener;

    use super::*;
    use crate::test_helpers::create_monitor_match;
    static TEST_SOCKET_COUNTER: AtomicU64 = AtomicU64::new(0);

    // Helper function to set up a mock server and a client pointing to it.
    async fn setup_test_client_and_server(handler: Router) -> (JsExecutorClient, PathBuf) {
        // Create a unique socket path for the test
        let count = TEST_SOCKET_COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let socket_path =
            env::temp_dir().join(format!("argus-test-js-executor-{}-{}.sock", pid, count));

        let listener = UnixListener::bind(&socket_path).unwrap();

        // Spawn the mock server in the background
        tokio::spawn(async move {
            axum::serve(listener, handler.into_make_service()).await.unwrap();
        });

        // Let the server start
        wait_for_socket(&socket_path).await.unwrap();

        // Configure the client to use this specific socket.
        let connector = UnixConnector { socket_path: socket_path.clone() };
        let hyper_client: Client<UnixConnector, Full<Bytes>> =
            Client::builder(hyper_util::rt::TokioExecutor::new()).build(connector);

        // Set executor_child to None because we are managing the server ourselves.
        let client = JsExecutorClient { executor_child: None, hyper_client };

        (client, socket_path)
    }

    #[tokio::test]
    async fn test_submit_script_success() {
        let context = create_monitor_match("test_notifier".to_string());
        let app = Router::new().route(
            "/execute",
            post(|| async {
                let response = ExecutionResponse {
                    result: serde_json::json!({"status": "success"}),
                    stdout: "Execution completed successfully".to_string(),
                    stderr: "".to_string(),
                };

                (StatusCode::OK, Json(response))
            }),
        );

        let (client, _socket_path) = setup_test_client_and_server(app).await;

        let result =
            client.submit_script("console.log('Hello, World!');".to_string(), &context).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.result, serde_json::json!({"status": "success"}));
        assert_eq!(response.stdout, "Execution completed successfully");
        assert_eq!(response.stderr, "");
    }

    #[tokio::test]
    async fn test_submit_script_server_error() {
        let context = create_monitor_match("test_notifier".to_string());
        let app = Router::new().route(
            "/execute",
            post(|| async {
                let error_response = ErrorResponse { error: "Script error".to_string() };
                (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response))
            }),
        );

        let (client, _socket_path) = setup_test_client_and_server(app).await;

        let result =
            client.submit_script("console.log('Hello, World!');".to_string(), &context).await;

        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(JsExecutorClientError::ScriptExecutionError(msg)) if msg == "Script error"
        ));
    }

    #[tokio::test]
    async fn test_submit_script_timeout() {
        let context = create_monitor_match("test_notifier".to_string());
        let app = Router::new().route(
            "/execute",
            post(|| async {
                // Simulate a delay longer than the client's timeout
                tokio::time::sleep(Duration::from_secs(REQUEST_TIMEOUT_SECONDS + 1)).await;
                (StatusCode::OK, Json(serde_json::json!({})))
            }),
        );
        let (client, _socket_path) = setup_test_client_and_server(app).await;
        let result =
            client.submit_script("console.log('Hello, World!');".to_string(), &context).await;

        assert!(result.is_err());
        assert!(matches!(result, Err(JsExecutorClientError::RequestTimeout)));
    }
}
