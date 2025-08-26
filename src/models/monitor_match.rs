//! This module defines the `MonitorMatch` struct.

use alloy::primitives::{Address, TxHash};
use serde::{Deserialize, Serialize};

/// Represents a match found by a monitor, containing detailed information
/// about the trigger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorMatch {
    /// Unique identifier for the monitor that found the match.
    pub monitor_id: i64,

    /// Name of the monitor that found the match.
    pub monitor_name: String,

    /// Name of the notifier that will handle notifications for this match.
    pub notifier_name: String,

    /// The data associated with the match, which can vary based on the type (e.g., transaction details, log details).
    pub match_data: MatchData,
}

impl MonitorMatch {
    /// Create MonitorMatch instance from transaction match data
    pub fn new_tx_match() -> Self {
        unimplemented!()
    }

    /// Create MonitorMatch instance from log match data
    pub fn new_log_match() -> Self {
        unimplemented!()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MatchData{
    Transaction(TransactionDetails),
    Log(LogDetails),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionDetails {
    /// The block number where the match was found.
    pub block_number: u64,
    
    /// The transaction hash associated with the match.
    pub transaction_hash: TxHash,

    /// Additional details about the transaction.
    pub details: serde_json::Value
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogDetails {
    /// The block number where the match was found.
    pub block_number: u64,
    
    /// The transaction hash associated with the match.
    pub transaction_hash: TxHash,

    /// The address of the contract that was the main subject of the monitor.
    pub contract_address: Address,

    /// The log index within the block, if the trigger was an event.
    pub log_index: Option<u64>,

    /// Additional details about the log/event.
    pub details: LogMatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogMatch {
    /// The name of the log/event.
    pub name: String,

    /// The parameters associated with the log/event.
    pub params: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monitor_match_construction_from_tx() {}

    #[test]
    fn test_monitor_match_construction_from_log() {}
}
