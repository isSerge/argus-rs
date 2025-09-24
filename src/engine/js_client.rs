//! Client for interacting with the JavaScript executor service.

use std::{process::Stdio, sync::Arc, time::Duration};

use common_models::{ExecutionRequest, ExecutionResponse};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::Mutex,
};

use crate::models::monitor_match::MonitorMatch;

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
    /// The port on which the JavaScript executor is listening.
    port: u16,
    /// HTTP client for making requests to the JavaScript executor.
    http_client: reqwest::Client,
}

/// Errors that can occur when interacting with the JavaScript executor
#[derive(Debug, thiserror::Error)]
pub enum JsExecutorClientError {
    /// Error spawning the JavaScript executor process.
    #[error("Failed to spawn JavaScript executor: {0}")]
    SpawnError(#[from] std::io::Error),

    /// Error reading the port from the JavaScript executor's stdout.
    #[error("Failed to read JavaScript executor port")]
    PortReadError,

    /// Error serializing the context for the JavaScript executor.
    #[error("Failed to serialize context for JavaScript executor: {0}")]
    SerializationFailed(#[from] serde_json::Error),

    /// Error making a request to the JavaScript executor.
    #[error("Failed to communicate with JavaScript executor: {0}")]
    ReqwestError(#[from] reqwest::Error),
}

impl JsExecutorClient {
    /// Creates a new instance of the JavaScript executor client.
    pub async fn new() -> Result<Self, JsExecutorClientError> {
        let mut cmd = Command::new("cargo");

        cmd.arg("run")
            .arg("-p")
            .arg("js_executor")
            .arg("--bin")
            .arg("js_executor")
            .stdout(Stdio::piped());

        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().expect("Failed to capture stdout");
        let mut reader = BufReader::new(stdout);

        let port_line = tokio::time::timeout(Duration::from_secs(5), async {
            let mut line = String::new();
            match reader.read_line(&mut line).await {
                Ok(0) => None, // EOF
                Ok(_) => Some(line),
                Err(_) => None,
            }
        })
        .await
        .map_err(|_| JsExecutorClientError::PortReadError)?
        .ok_or(JsExecutorClientError::PortReadError)?;

        // Extract the port number from the line "Listening on <port>"
        let port: u16 = port_line
            .trim()
            .parse()
            .map_err(|_| JsExecutorClientError::PortReadError)?;

        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECONDS))
            .build()
            .expect("Failed to build HTTP client");

        Ok(Self { executor_child: Arc::new(Mutex::new(child)), port, http_client })
    }

    /// Returns the port on which the JavaScript executor is listening.
    pub fn port(&self) -> u16 {
        self.port
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
        let url = format!("http://127.0.0.1:{}/execute", self.port);
        let context = serde_json::to_value(context)?;
        let request = ExecutionRequest { script, context };
        let response = self.http_client.post(&url).json(&request).send().await?;
        let json = response.json::<ExecutionResponse>().await?;
        Ok(json)
    }
}

impl Drop for JsExecutorClient {
    fn drop(&mut self) {
        let executor_child = self.executor_child.clone();
        tokio::spawn(async move {
            let mut child = executor_child.lock().await;
            if let Err(e) = child.kill().await {
                tracing::error!("Failed to kill JavaScript executor process: {}", e);
            }
        });
    }
}
