//! This module defines the `MonitorMatch` struct.

/// Represents a match found by a monitor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorMatch {
    // Placeholder fields for now
    pub monitor_id: String,
    pub block_number: u64,
    pub transaction_hash: alloy::primitives::TxHash,
    pub log_index: Option<u64>,
    pub data: String,
}
