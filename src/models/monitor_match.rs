//! This module defines the `MonitorMatch` struct.

/// Represents a match found by a monitor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorMatch {
    /// Unique identifier for the monitor that found the match.
    pub monitor_id: String,
    /// The block number where the match was found.
    pub block_number: u64,
    /// The transaction hash associated with the match.
    pub transaction_hash: alloy::primitives::TxHash,
    /// The log index within the transaction where the match was found.
    pub log_index: Option<u64>,
    /// Additional data associated with the match.
    pub data: String,
}
