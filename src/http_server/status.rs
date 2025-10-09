//! Represents the response structure for the `/status` endpoint in the HTTP server.
use serde::Serialize;

/// Represents the response from the `/status` endpoint.
#[derive(Debug, Serialize, Clone)]
pub struct StatusResponse {
    /// The version of the application.
    pub version: String,
    /// The network ID the application is connected to.
    pub network_id: String,
    /// The uptime of the application in seconds.
    pub uptime_secs: u64,
    /// The latest block number that has been processed.
    pub latest_processed_block: u64,
    /// The timestamp of the latest processed block in seconds.
    pub latest_processed_block_timestamp_secs: u64,
}
