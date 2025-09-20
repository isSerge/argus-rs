//! JavaScript action runner.

use std::sync::Arc;

use deno_core::{JsRuntime, RuntimeOptions};
use thiserror::Error;

use crate::{config::ActionConfig, models::monitor_match::MonitorMatch};

/// An error that occurs during JavaScript script execution.
#[derive(Debug, Error)]
pub enum JsRunnerError {
    /// An error occurred while reading the action file.
    #[error("Failed to read action file '{file_path}': {error}")]
    FileRead { file_path: String, error: std::io::Error },

    /// An error occurred during script execution.
    #[error("Script execution error: {0}")]
    ScriptExecution(#[from] Box<deno_core::error::JsError>),

    /// An error occurred during serialization or deserialization.
    #[error("Serialization/Deserialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// An error occurred in the runtime task.
    #[error("Runtime task error: {0}")]
    Runtime(#[from] Box<dyn std::error::Error + Send + Sync>),
}

/// A runner for executing JavaScript actions.
#[derive(Clone)]
pub struct JavaScriptRunner {
    // Using Arc for cheap cloning and shared state if needed in the future.
    _inner: Arc<Inner>,
}

struct Inner;

impl JavaScriptRunner {
    /// Creates a new `JavaScriptRunner`.
    pub fn new() -> Self {
        Self { _inner: Arc::new(Inner) }
    }

    /// Executes a JavaScript action.
    pub async fn execute_action(
        &self,
        action: &ActionConfig,
        context: &MonitorMatch,
    ) -> Result<(), JsRunnerError> {
        let action_file = action.file.clone();
        let context_json = serde_json::to_string(context)?;

        // Use spawn_blocking to move JavaScript runtime operations to the blocking
        // thread pool
        tokio::task::spawn_blocking(move || -> Result<(), JsRunnerError> {
            // Create runtime in the blocking thread
            let mut runtime = JsRuntime::new(RuntimeOptions::default());

            // Read the script file synchronously in the blocking thread
            let script =
                std::fs::read_to_string(&action_file).map_err(|e| JsRunnerError::FileRead {
                    file_path: action_file.to_string_lossy().to_string(),
                    error: e,
                })?;

            // Bootstrap the runtime with the context
            let bootstrap_script = format!("const match = {};", context_json);
            runtime
                .execute_script("<bootstrap>", bootstrap_script)
                .map_err(JsRunnerError::ScriptExecution)?;

            // Execute the user's action script
            let result = runtime.execute_script("<action>", script);

            if let Err(e) = result {
                tracing::error!(
                    "Error executing action file '{}': {}",
                    action_file.to_string_lossy(),
                    e
                );
                return Err(JsRunnerError::ScriptExecution(e));
            }

            Ok(())
        })
        .await
        .map_err(|e| JsRunnerError::Runtime(Box::new(e)))??;

        Ok(())
    }
}

impl Default for JavaScriptRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use alloy::primitives::TxHash;
    use serde_json::json;
    use tempfile::NamedTempFile;

    use super::*;
    use crate::models::monitor_match::{MatchData, TransactionMatchData};

    fn create_test_action_file(content: &str) -> (NamedTempFile, ActionConfig) {
        let mut file = NamedTempFile::with_suffix(".js").unwrap();
        let path = file.path().to_path_buf();
        write!(file, "{}", content).unwrap();
        let action_config = ActionConfig { name: "test_action".to_string(), file: path };
        (file, action_config)
    }

    fn create_test_monitor_match() -> MonitorMatch {
        MonitorMatch {
            monitor_id: 1,
            monitor_name: "Test Monitor".to_string(),
            notifier_name: "test-notifier".to_string(),
            block_number: 123,
            transaction_hash: TxHash::default(),
            match_data: MatchData::Transaction(TransactionMatchData {
                details: json!({"foo": "bar"}),
            }),
        }
    }

    #[tokio::test]
    async fn test_execute_action_success() {
        let (_file, action) = create_test_action_file("console.log('hello');");
        let context = create_test_monitor_match();
        let runner = JavaScriptRunner::new();

        let result = runner.execute_action(&action, &context).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_action_script_error() {
        let (_file, action) = create_test_action_file("throw new Error('test error');");
        let context = create_test_monitor_match();
        let runner = JavaScriptRunner::new();

        let result = runner.execute_action(&action, &context).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            JsRunnerError::ScriptExecution(e) => {
                assert!(e.to_string().contains("test error"));
            }
            _ => panic!("Expected ScriptExecution"),
        }
    }

    #[tokio::test]
    async fn test_execute_action_with_context() {
        let script = r#"
            if (match.monitor_id !== 1) {
                throw new Error("wrong monitor id");
            }
            if (match.tx.foo !== "bar") {
                throw new Error("wrong tx details");
            }
        "#;
        let (_file, action) = create_test_action_file(script);
        let context = create_test_monitor_match();
        let runner = JavaScriptRunner::new();

        let result = runner.execute_action(&action, &context).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_action_file_not_found() {
        let action =
            ActionConfig { name: "test_action".to_string(), file: "non_existent_file.js".into() };
        let context = create_test_monitor_match();
        let runner = JavaScriptRunner::new();

        let result = runner.execute_action(&action, &context).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            JsRunnerError::FileRead { .. } => {
                // expected
            }
            _ => panic!("Expected FileRead"),
        }
    }
}
