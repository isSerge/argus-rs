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

    /// The JavaScript executor failed to start up properly.
    #[error("JavaScript executor failed to start up")]
    StartupFailed,
}

impl JsExecutorClient {
    /// Creates a new instance of the JavaScript executor client.
    pub async fn new() -> Result<Self, JsExecutorClientError> {
        // Generate a unique socket path for this client instance
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

        // Pass the socket path to the js_executor
        cmd.arg("--socket-path").arg(&socket_path);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().expect("Failed to capture stdout");
        let stderr = child.stderr.take().expect("Failed to capture stderr");

        // Spawn a simple background task to drain stdout/stderr
        tokio::spawn(async move {
            let mut stdout_reader = BufReader::new(stdout);
            let mut stderr_reader = BufReader::new(stderr);
            loop {
                let mut stdout_line = String::new();
                let mut stderr_line = String::new();
                tokio::select! {
                    res = stdout_reader.read_line(&mut stdout_line) => {
                        if res.unwrap_or(0) == 0 { break; }
                        tracing::debug!(target: "js_executor", "{}", stdout_line.trim());
                    },
                    res = stderr_reader.read_line(&mut stderr_line) => {
                        if res.unwrap_or(0) == 0 { break; }
                        tracing::warn!(target: "js_executor", "{}", stderr_line.trim());
                    },
                }
            }
        });

        // Build the hyper client
        let connector = UnixConnector { socket_path: socket_path.clone() };
        let hyper_client: Client<UnixConnector, Full<Bytes>> =
            Client::builder(hyper_util::rt::TokioExecutor::new()).build(connector);

        // Wait for the socket to become available with exponential backoff
        let socket_ready = async {
            for attempt in 0..10 {
                if tokio::net::UnixStream::connect(&socket_path).await.is_ok() {
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(50 << attempt.min(6))).await;
            }
            Err(JsExecutorClientError::StartupFailed)
        };
        
        socket_ready.await?;
        tracing::debug!("JavaScript executor socket is ready at {}", socket_path.display());

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
