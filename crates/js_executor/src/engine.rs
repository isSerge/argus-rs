//! JavaScript action
use common_models::ExecutionResponse;
use deno_core::{JsRuntime, RuntimeOptions, serde_v8, v8};
use serde_json::Value;
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

/// Executes a JavaScript script and returns the modified Value.
pub async fn execute_script(
    script: String,
    context: &Value,
) -> Result<ExecutionResponse, JsRunnerError> {
    let context_json = serde_json::to_string(context)?;

    // Use spawn_blocking to move JavaScript runtime operations to the blocking
    // thread pool
    let modified_ctx_value =
        tokio::task::spawn_blocking(move || -> Result<Value, JsRunnerError> {
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
            let modified_ctx_value: Value = serde_v8::from_v8(scope, local)?;

            Ok(modified_ctx_value)
        })
        .await
        .map_err(|e| JsRunnerError::Runtime(Box::new(e)))??;

    Ok(ExecutionResponse {
        result: modified_ctx_value,
        stdout: "".to_string(), // Placeholder, as console output capture is not implemented
        stderr: "".to_string(), // Placeholder, as console output capture is not implemented
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn create_test_context_json() -> Value {
        json!({
            "monitor_id": 1,
            "monitor_name": "Test Monitor",
            "notifier_name": "test-notifier",
        })
    }

    #[tokio::test]
    async fn test_execute_script_success() {
        let script = r#"
            console.log("Test script executed");
            // Modify the match object
            match.monitor_name = match.monitor_name + "[modified]";
        "#
        .to_string();
        let context = create_test_context_json();

        let exec_response = execute_script(script, &context).await;
        assert!(exec_response.is_ok());
        let modified_context = exec_response.unwrap().result;
        assert_eq!(modified_context["monitor_name"], "Test Monitor[modified]");
        // Other fields should remain unchanged
        assert_eq!(modified_context["monitor_id"], context["monitor_id"]);
        assert_eq!(modified_context["notifier_name"], context["notifier_name"]);
    }

    #[tokio::test]
    async fn test_execute_script_script_error() {
        let script = r#"
            throw new Error("test error");
        "#
        .to_string();
        let context = create_test_context_json();

        let result = execute_script(script, &context).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            JsRunnerError::ScriptExecution(e) => {
                assert!(e.to_string().contains("test error"));
            }
            _ => panic!("Expected ScriptExecution"),
        }
    }
}
