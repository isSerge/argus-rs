//! This module defines the `MonitorMatch` struct.

use alloy::primitives::{Address, TxHash};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Represents a match found by a monitor, containing detailed information
/// about the trigger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorMatch {
    /// Unique identifier for the monitor that found the match.
    pub monitor_id: i64,
    /// The block number where the match was found.
    pub block_number: u64,
    /// The transaction hash associated with the match.
    pub transaction_hash: TxHash,
    /// The address of the contract that was the main subject of the monitor.
    pub contract_address: Address,
    /// The specific trigger that caused the match (e.g., an event name or function signature).
    pub trigger_name: String,
    /// The decoded data of the event or transaction that matched, as a JSON object.
    pub trigger_data: Value,
    /// The log index within the block, if the trigger was an event.
    pub log_index: Option<u64>,
}
