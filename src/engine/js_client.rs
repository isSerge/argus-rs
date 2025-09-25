//! Client for interacting with the JavaScript executor service.

use std::{
    env,
    future::Future,
    path::PathBuf,
    pin::Pin,
    process::Stdio,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use common_models::{ExecutionRequest, ExecutionResponse};
use http_body_util::Full;
use hyper::{Uri, body::Bytes};
use hyper_util::{client::legacy::Client, rt::TokioIo};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    net::UnixStream,
    process::{Child, Command},
    sync::Mutex,
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
// Increased startup timeout for potentially slow CI environments
const STARTUP_TIMEOUT_SECONDS: u64 = 180;

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
    executor_child: Arc<Mutex<Child>>,
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

    /// Error reading the socket path from the JavaScript executor's stdout.
    #[error(
        "Failed to read JavaScript executor socket path. The process may have exited \
         early.\n---\n{stdout}\n--- STDERR ---\n{stderr}"
    )]
    SocketPathReadError {
        /// Captured stdout from the executor process.
        stdout: String,
        /// Captured stderr from the executor process.
        stderr: String,
    },

    /// The JavaScript executor took too long to start up and signal its socket
    /// path.
    #[error(
        "JavaScript executor timed out after {timeout:?} during startup.\n--- STDOUT \
         ---\n{stdout}\n--- STDERR ---\n{stderr}"
    )]
    StartupTimeout {
        /// Duration after which the startup timed out.
        timeout: Duration,
        /// Captured stdout from the executor process.
        stdout: String,
        /// Captured stderr from the executor process.
        stderr: String,
    },
}

impl JsExecutorClient {
    /// Creates a new instance of the JavaScript executor client.
    pub async fn new() -> Result<Self, JsExecutorClientError> {
        // 1. Generate a unique socket path for this client instance.
        let socket_path =
            env::temp_dir().join(format!("argus-js-executor-{}.sock", std::process::id()));

        let mut cmd = if let Ok(bin_path) = env::var("JS_EXECUTOR_BIN_PATH") {
            Command::new(bin_path)
        } else if let Ok(bin_path) = env::var("CARGO_BIN_EXE_js_executor") {
            Command::new(bin_path)
        } else {
            let mut cmd = Command::new("cargo");
            cmd.arg("run").arg("-p").arg("js_executor").arg("--bin").arg("js_executor").arg("--");
            cmd
        };

        // 2. Pass the generated socket path to the child process as an argument.
        cmd.arg("--socket-path").arg(&socket_path);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().expect("Failed to capture stdout");
        let stderr = child.stderr.take().expect("Failed to capture stderr");

        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<()>();
        let stdout_output = Arc::new(Mutex::new(String::new()));
        let stderr_output = Arc::new(Mutex::new(String::new()));
        let stdout_clone = stdout_output.clone();
        let stderr_clone = stderr_output.clone();

        // 3. Spawn a background task to wait for the "Listening on" message
        tokio::spawn(async move {
            let mut stdout_reader = BufReader::new(stdout);
            let mut stderr_reader = BufReader::new(stderr);
            let mut ready_tx = Some(ready_tx);

            const READY_MARKER: &str = "Listening on ";

            loop {
                let mut stdout_line = String::new();
                let mut stderr_line = String::new();
                tokio::select! {
                    res = stdout_reader.read_line(&mut stdout_line) => {
                        match res {
                            Ok(0) => break, // EOF
                            Ok(_) => {
                                tracing::debug!(target: "js_executor", "{}", stdout_line.trim());
                                stdout_clone.lock().await.push_str(&stdout_line);
                                
                                // Check if this is the ready signal
                                if let Some(tx) = ready_tx.take() {
                                    if stdout_line.trim().starts_with(READY_MARKER) {
                                        let _ = tx.send(());
                                    } else {
                                        ready_tx = Some(tx);
                                    }
                                }
                            },
                            Err(e) => {
                                tracing::error!("Error reading js_executor stdout: {}", e);
                                break;
                            }
                        }
                    },
                    res = stderr_reader.read_line(&mut stderr_line) => {
                        match res {
                            Ok(0) => {}, // EOF
                            Ok(_) => {
                                tracing::warn!(target: "js_executor", "{}", stderr_line.trim());
                                stderr_clone.lock().await.push_str(&stderr_line);
                            },
                            Err(e) => {
                                tracing::error!("Error reading js_executor stderr: {}", e);
                            }
                        }
                    }
                }
            }
        });        
        
        // 4. Wait for the executor to signal it's ready
        let startup_timeout = Duration::from_secs(STARTUP_TIMEOUT_SECONDS);
        match tokio::time::timeout(startup_timeout, ready_rx).await {
            Err(_) => {
                return Err(JsExecutorClientError::StartupTimeout {
                    timeout: startup_timeout,
                    stdout: stdout_output.lock().await.clone(),
                    stderr: stderr_output.lock().await.clone(),
                });
            }
            Ok(Err(_)) => {
                return Err(JsExecutorClientError::SocketPathReadError {
                    stdout: stdout_output.lock().await.clone(),
                    stderr: stderr_output.lock().await.clone(),
                });
            }
            Ok(Ok(())) => {
                tracing::info!("JavaScript executor is ready at {}", socket_path.display());
            }
        }

        // 5. Build the hyper client now that we know the executor is ready
        let connector = UnixConnector { socket_path };
        let hyper_client: Client<UnixConnector, Full<Bytes>> =
            Client::builder(hyper_util::rt::TokioExecutor::new()).build(connector);

        Ok(Self { executor_child: Arc::new(Mutex::new(child)), hyper_client })
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
            .uri("http://unix/execute")
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(body)))?;

        // Send the request with a timeout
        let response = match tokio::time::timeout(
            Duration::from_secs(REQUEST_TIMEOUT_SECONDS),
            self.hyper_client.request(req),
        )
        .await
        {
            Ok(Ok(res)) => res,
            Ok(Err(e)) => return Err(JsExecutorClientError::ClientError(e)),
            Err(_) => return Err(JsExecutorClientError::RequestTimeout),
        };

        // Read the response body
        let body_bytes = http_body_util::BodyExt::collect(response.into_body()).await?.to_bytes();
        let json: ExecutionResponse = serde_json::from_slice(&body_bytes)?;

        Ok(json)
    }
}

impl Drop for JsExecutorClient {
    fn drop(&mut self) {
        tracing::debug!("Dropping JsExecutorClient");
        let executor_child = self.executor_child.clone();
        tokio::spawn(async move {
            let mut child = executor_child.lock().await;
            if let Err(e) = child.kill().await {
                tracing::error!("Failed to kill JavaScript executor process: {}", e);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    // TODO: Implement tests for JsExecutorClient using a real axum server and UDS
}
