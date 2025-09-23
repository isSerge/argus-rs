//! JavaScript action
use argus_models::monitor_match::MonitorMatch;
use deno_core::{JsRuntime, RuntimeOptions, serde_v8, v8};
use thiserror::Error;

/// An error that occurs during JavaScript script execution.
#[derive(Debug, Error)]
pub enum JsRunnerError {
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

/// Executes a JavaScript script and returns the modified MonitorMatch.
pub async fn execute_script(
    script: String,
    context: &MonitorMatch,
) -> Result<MonitorMatch, JsRunnerError> {
    let context_json = serde_json::to_string(context)?;

    // Use spawn_blocking to move JavaScript runtime operations to the blocking
    // thread pool
    let modified_match =
        tokio::task::spawn_blocking(move || -> Result<MonitorMatch, JsRunnerError> {
            // Create runtime in the blocking thread
            let mut runtime = JsRuntime::new(RuntimeOptions::default());

            // Load the console polyfill
            let console_polyfill = include_str!("./console_polyfill.js");
            runtime
                .execute_script("<console_polyfill>", console_polyfill)
                .map_err(JsRunnerError::ScriptExecution)?;

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

    Ok(modified_match)
}

#[cfg(test)]
mod tests {
    use alloy::primitives::TxHash;
    use argus_models::monitor_match::{MatchData, TransactionMatchData};
    use serde_json::json;

    use super::*;

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
    async fn test_execute_script_success() {
        let script = r#"
            console.log("Test script executed");
            // Modify the match object
            match.monitor_name = match.monitor_name + "[modified]";
        "#
        .to_string();
        let context = create_test_monitor_match();

        let result = execute_script(script, &context).await;
        assert!(result.is_ok());
        let modified_match = result.unwrap();
        assert_eq!(modified_match.monitor_id, context.monitor_id);
        assert_eq!(modified_match.monitor_name, "Test Monitor[modified]");
    }

    #[tokio::test]
    async fn test_execute_script_script_error() {
        let script = r#"
            throw new Error("test error");
        "#
        .to_string();
        let context = create_test_monitor_match();

        let result = execute_script(script, &context).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            JsRunnerError::ScriptExecution(e) => {
                assert!(e.to_string().contains("test error"));
            }
            _ => panic!("Expected ScriptExecution"),
        }
    }

    #[tokio::test]
    async fn test_execute_script_with_context() {
        let script = r#"
            if (match.monitor_id !== 1) {
                throw new Error("wrong monitor id");
            }
            console.log("Test passed with context:", match.monitor_id);
            
            // Modify the match object directly
            match.monitor_name = "Modified Monitor";
        "#
        .to_string();
        let context = create_test_monitor_match();

        let result = execute_script(script, &context).await;
        assert!(result.is_ok());
        let modified_match = result.unwrap();
        assert_eq!(modified_match.monitor_name, "Modified Monitor");
        assert_eq!(modified_match.monitor_id, context.monitor_id);
    }
}
