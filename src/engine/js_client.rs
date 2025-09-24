//! Client for interacting with the JavaScript executor service.

use std::{env, process::Stdio, sync::Arc, time::Duration};

use common_models::{ExecutionRequest, ExecutionResponse};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::Mutex,
};

use crate::models::monitor_match::MonitorMatch;

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
    #[error(
        "Failed to read JavaScript executor port. The process may have exited early.\n--- STDOUT \
         ---\n{stdout}\n--- STDERR ---\n{stderr}"
    )]
    PortReadError {
        /// Captured stdout from the executor process.
        stdout: String,
        /// Captured stderr from the executor process.
        stderr: String,
    },

    /// The JavaScript executor took too long to start up and signal its port.
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
        let mut cmd = if let Ok(bin_path) = env::var("JS_EXECUTOR_BIN_PATH") {
            // PRIMARY METHOD: Use our manually set, reliable path from CI.
            Command::new(bin_path)
        } else if let Ok(bin_path) = env::var("CARGO_BIN_EXE_js_executor") {
            // SECONDARY METHOD: Keep the idiomatic Cargo way for local tests.
            Command::new(bin_path)
        } else {
            // FINAL FALLBACK: For local dev or if all else fails.
            let mut cmd = Command::new("cargo");
            cmd.arg("run").arg("-p").arg("js_executor").arg("--bin").arg("js_executor");
            cmd
        };

        // Capture stdout to read the port number and stderr for logging
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().expect("Failed to capture stdout");
        let stderr = child.stderr.take().expect("Failed to capture stderr");

        let mut stdout_reader = BufReader::new(stdout);
        let mut stderr_reader = BufReader::new(stderr);

        const READY_MARKER: &str = "Listening on ";
        let startup_timeout = Duration::from_secs(STARTUP_TIMEOUT_SECONDS);

        let mut stdout_output = String::new();
        let mut stderr_output = String::new();

        let timeout_result = tokio::time::timeout(startup_timeout, async {
            loop {
                let mut stdout_line = String::new();
                let mut stderr_line = String::new();

                tokio::select! {
                    // Read a line from stdout
                    res = stdout_reader.read_line(&mut stdout_line) => {
                        match res {
                            Ok(0) => break None, // EOF
                            Ok(_) => {
                                tracing::debug!("js_executor stdout: {}", stdout_line.trim());
                                stdout_output.push_str(&stdout_line);
                                if let Some(port_str) = stdout_line.trim().strip_prefix(READY_MARKER)
                                    && let Ok(p) = port_str.parse::<u16>() {
                                        break Some(p); // Success
                                    }
                            }
                            Err(e) => {
                                tracing::error!("Error reading js_executor stdout: {}", e);
                                break None;
                            }
                        }
                    },
                    // Read a line from stderr
                    res = stderr_reader.read_line(&mut stderr_line) => {
                         match res {
                            Ok(0) => {}, // EOF on stderr is fine
                            Ok(_) => {
                                tracing::warn!("js_executor stderr: {}", stderr_line.trim());
                                stderr_output.push_str(&stderr_line);
                            },
                            Err(e) => {
                                tracing::error!("Error reading js_executor stderr: {}", e);
                            }
                        }
                    }
                }
            }
        })
        .await;

        let port = match timeout_result {
            // Timeout occurred
            Err(_) => {
                return Err(JsExecutorClientError::StartupTimeout {
                    timeout: startup_timeout,
                    stdout: stdout_output,
                    stderr: stderr_output,
                });
            }
            // Timeout did not occur, check inner result
            Ok(Some(p)) => p, // Success
            Ok(None) => {
                // The async block completed but returned None (e.g. stdout closed)
                return Err(JsExecutorClientError::PortReadError {
                    stdout: stdout_output,
                    stderr: stderr_output,
                });
            }
        };

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
