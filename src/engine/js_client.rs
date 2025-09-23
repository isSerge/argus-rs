//! Client for interacting with the JavaScript executor service.

use std::{process::Stdio, sync::Arc};

use crate::models::{monitor_match::MonitorMatch};
use js_executor::{ExecutionRequest, ExecutionResponse};

use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::Mutex,
};

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

    /// Error communicating with the JavaScript executor.
    #[error("JavaScript executor process exited unexpectedly")]
    ProcessExit,
}

impl JsExecutorClient {
    /// Creates a new instance of the JavaScript executor client.
    pub async fn new() -> Result<Self, JsExecutorClientError> {
        let mut cmd = Command::new("cargo");

        cmd.arg("run").arg("--bin").arg("js_executor").stdout(Stdio::piped());

        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().expect("Failed to capture stdout");

        let reader = BufReader::new(stdout);

        let port_line =
            reader.lines().next_line().await?.ok_or(JsExecutorClientError::PortReadError)?;

        let port: u16 =
            port_line.trim().parse().map_err(|_| JsExecutorClientError::PortReadError)?;

        Ok(Self {
            executor_child: Arc::new(Mutex::new(child)),
            port,
            http_client: reqwest::Client::new(),
        })
    }

    /// Returns the port on which the JavaScript executor is listening.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Submits a JavaScript script to the executor for execution.
    pub async fn submit_script(
        &self,
        script: &str,
        context: MonitorMatch,
    ) -> Result<ExecutionResponse, reqwest::Error> {
        let url = format!("http://localhost:{}/execute", self.port);
        let request = ExecutionRequest {
            script: script.to_string(),
            context: serde_json::to_value(context).unwrap(),
        };
        let response = self.http_client.post(&url).json(&request).send().await?;
        response.json::<ExecutionResponse>().await
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
