//! Common data models for inter-service communication between Argus and JS
//! executor.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Request payload for executing a JavaScript script.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExecutionRequest {
    /// The JavaScript code to execute.
    pub script: String,
    /// The context to pass into the script.
    pub context: Value,
}

/// Response payload for a successful script execution.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExecutionResponse {
    /// The result of the script execution.
    pub result: Value,
    /// The standard output produced by the script.
    pub stdout: String,
    /// The standard error produced by the script.
    pub stderr: String,
}

/// Response payload for an error during script execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
}
