//! JavaScript action
use deno_core::{JsRuntime, RuntimeOptions, serde_v8, v8};
use thiserror::Error;

use crate::{config::ActionConfig, models::monitor_match::MonitorMatch};

/// An error that occurs during JavaScript script execution.
#[derive(Debug, Error)]
pub enum JsRunnerError {
    /// An error occurred while reading the action file.
    #[error("Failed to read action file '{file_path}': {error}")]
    FileRead {
        /// The path to the action file that failed to read.
        file_path: String,
        /// The underlying I/O error that occurred.
        error: std::io::Error,
    },

    /// An error occurred during script execution.
    #[error("Script execution error: {0}")]
    ScriptExecution(#[from] Box<deno_core::error::JsError>),

    /// An error occurred during serialization or deserialization.
    #[error("Serialization/Deserialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// An error occurred during V8 serialization.
    #[error("V8 serialization error: {0}")]
    SerdeV8Error(#[from] serde_v8::Error),

    /// An error occurred in the runtime task.
    #[error("Runtime task error: {0}")]
    Runtime(#[from] Box<dyn std::error::Error + Send + Sync>),
}

/// Executes a JavaScript action and returns the modified MonitorMatch.
pub async fn execute_action(
    action: &ActionConfig,
    context: &MonitorMatch,
) -> Result<MonitorMatch, JsRunnerError> {
    let action_file = action.file.clone();
    let context_json = serde_json::to_string(context)?;

    // Use spawn_blocking to move JavaScript runtime operations to the blocking
    // thread pool
    let modified_match =
        tokio::task::spawn_blocking(move || -> Result<MonitorMatch, JsRunnerError> {
            // Create runtime in the blocking thread
            let mut runtime = JsRuntime::new(RuntimeOptions::default());

            // Load the console polyfill
            let console_polyfill = include_str!("../../js/console_polyfill.js");
            runtime
                .execute_script("<console_polyfill>", console_polyfill)
                .map_err(JsRunnerError::ScriptExecution)?;

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
            runtime.execute_script("<action>", script).map_err(JsRunnerError::ScriptExecution)?;

            // Capture the final state of the match object
            let result_script = "match;";

            let return_value = runtime
                .execute_script("<get_result>", result_script)
                .map_err(JsRunnerError::ScriptExecution)?;

            // Convert the result back to Rust
            let scope = &mut runtime.handle_scope();
            let local = v8::Local::new(scope, return_value);
            let modified_match: MonitorMatch = serde_v8::from_v8(scope, local)?;

            Ok(modified_match)
        })
        .await
        .map_err(|e| JsRunnerError::Runtime(Box::new(e)))??;

    println!("Modified MonitorMatch: {:?}", modified_match);
    Ok(modified_match)
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

        let result = execute_action(&action, &context).await;
        assert!(result.is_ok());
        let modified_match = result.unwrap();
        assert_eq!(modified_match.monitor_id, context.monitor_id);
    }

    #[tokio::test]
    async fn test_execute_action_script_error() {
        let (_file, action) = create_test_action_file("throw new Error('test error');");
        let context = create_test_monitor_match();

        let result = execute_action(&action, &context).await;
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
            console.log("Test passed with context:", match.monitor_id);
            
            // Modify the match object directly
            match.monitor_name = "Modified Monitor";
        "#;
        let (_file, action) = create_test_action_file(script);
        let context = create_test_monitor_match();

        let result = execute_action(&action, &context).await;
        assert!(result.is_ok());
        let modified_match = result.unwrap();
        assert_eq!(modified_match.monitor_name, "Modified Monitor");
        assert_eq!(modified_match.monitor_id, context.monitor_id);
    }

    #[tokio::test]
    async fn test_execute_action_file_not_found() {
        let action =
            ActionConfig { name: "test_action".to_string(), file: "non_existent_file.js".into() };
        let context = create_test_monitor_match();

        let result = execute_action(&action, &context).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            JsRunnerError::FileRead { .. } => {
                // expected
            }
            _ => panic!("Expected FileRead"),
        }
    }
}
