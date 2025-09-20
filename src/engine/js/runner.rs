//! JavaScript action runner.

use std::sync::Arc;

use deno_core::{JsRuntime, RuntimeOptions};
use thiserror::Error;
use tokio::fs;

use crate::{config::ActionConfig, models::monitor_match::MonitorMatch};

/// An error that occurs during JavaScript script execution.
#[derive(Debug, Error)]
pub enum JsRunnerError {
    /// An error occurred while reading the action file.
    #[error("Failed to read action file '{file_path}': {error}")]
    FileReadError { file_path: String, error: std::io::Error },

    /// An error occurred during script execution.
    #[error("Script execution error: {0}")]
    ScriptExecutionError(#[from] Box<deno_core::error::JsError>),

    /// An error occurred during serialization or deserialization.
    #[error("Serialization/Deserialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
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
        let mut runtime = JsRuntime::new(RuntimeOptions::default());

        let script =
            fs::read_to_string(&action.file).await.map_err(|e| JsRunnerError::FileReadError {
                file_path: action.file.to_string_lossy().to_string(),
                error: e,
            })?;

        // Serialize the context to a JSON string
        let context_json = serde_json::to_string(context)?;

        // Bootstrap the runtime with the context
        let bootstrap_script = format!("const match = {};", context_json);
        runtime.execute_script("<bootstrap>", bootstrap_script)?;

        // Execute the user's action script
        let result = runtime.execute_script("<action>", script);

        if let Err(e) = result {
            tracing::error!(
                "Error executing action file '{}': {}",
                action.file.to_string_lossy(),
                e
            );
            return Err(e.into());
        }

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
            JsRunnerError::ScriptExecutionError(e) => {
                assert!(e.to_string().contains("test error"));
            }
            _ => panic!("Expected ScriptExecutionError"),
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
            JsRunnerError::FileReadError { .. } => {
                // expected
            }
            _ => panic!("Expected FileReadError"),
        }
    }
}
