use serde::{Deserialize, Serialize};

use crate::monitor_match::MonitorMatch;

/// Request payload for executing a JavaScript script.
#[derive(Deserialize)]
pub struct ExecutionRequest {
    /// The JavaScript code to execute.
    pub script: String,
    /// The context to pass into the script.
    pub context: MonitorMatch,
}

/// Response payload for a successful script execution.
#[derive(Serialize)]
pub struct ExecutionResponse {
    pub result: MonitorMatch,
}

/// Response payload for an error during script execution.
#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}
