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

    /// The data associated with the match, which can vary based on the type
    /// (e.g., transaction details, log details).
    pub match_data: MatchData,
}

impl MonitorMatch {
    /// Create MonitorMatch instance from transaction match data
    pub fn new_tx_match(
        monitor_id: i64,
        monitor_name: String,
        notifier_name: String,
        block_number: u64,
        transaction_hash: TxHash,
        details: serde_json::Value,
    ) -> Self {
        Self {
            monitor_id,
            monitor_name,
            notifier_name,
            match_data: MatchData::Transaction(TransactionDetails {
                block_number,
                transaction_hash,
                details,
            }),
        }
    }

    /// Create MonitorMatch instance from log match data
    pub fn new_log_match(
        monitor_id: i64,
        monitor_name: String,
        notifier_name: String,
        block_number: u64,
        transaction_hash: TxHash,
        contract_address: Address,
        log_index: u64,
        log_name: String,
        log_params: serde_json::Value,
    ) -> Self {
        Self {
            monitor_id,
            monitor_name,
            notifier_name,
            match_data: MatchData::Log(LogDetails {
                block_number,
                transaction_hash,
                contract_address,
                log_index,
                details: LogMatch {
                    name: log_name,
                    params: log_params,
                },
            }),
        }
    }
}

/// Represents the data associated with a monitor match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MatchData {
    /// A transaction match.
    Transaction(TransactionDetails),
    /// A log/event match.
    Log(LogDetails),
}

/// Details about a transaction match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionDetails {
    /// The block number where the match was found.
    pub block_number: u64,

    /// The transaction hash associated with the match.
    pub transaction_hash: TxHash,

    /// Additional details about the transaction.
    pub details: serde_json::Value,
}

/// Details about a log/event match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogDetails {
    /// The block number where the match was found.
    pub block_number: u64,

    /// The transaction hash associated with the match.
    pub transaction_hash: TxHash,

    /// The address of the contract that was the main subject of the monitor.
    pub contract_address: Address,

    /// The log index within the block, if the trigger was an event.
    pub log_index: u64,

    /// Additional details about the log/event.
    pub details: LogMatch,
}

/// Details about a log/event match.
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
    fn test_monitor_match_construction_from_tx() {
        let monitor_match = MonitorMatch::new_tx_match(
            1,
            "Test Monitor".to_string(),
            "Test Notifier".to_string(),
            123,
            TxHash::default(),
            serde_json::json!({"key": "value"}),
        );

        assert_eq!(monitor_match.monitor_id, 1);
        assert_eq!(monitor_match.monitor_name, "Test Monitor");
        assert_eq!(monitor_match.notifier_name, "Test Notifier");

        if let MatchData::Transaction(tx_details) = monitor_match.match_data {
            assert_eq!(tx_details.block_number, 123);
            assert_eq!(tx_details.transaction_hash, TxHash::default());
            assert_eq!(tx_details.details, serde_json::json!({"key": "value"}));
        } else {
            panic!("Expected MatchData::Transaction variant");
        }
    }

    #[test]
    fn test_monitor_match_construction_from_log() {
        let monitor_match = MonitorMatch::new_log_match(
            2,
            "Log Monitor".to_string(),
            "Log Notifier".to_string(),
            456,
            TxHash::default(),
            Address::default(),
            15,
            "TestLog".to_string(),
            serde_json::json!({"param1": "value1", "param2": 42}),
        );

        assert_eq!(monitor_match.monitor_id, 2);
        assert_eq!(monitor_match.monitor_name, "Log Monitor");
        assert_eq!(monitor_match.notifier_name, "Log Notifier");

        if let MatchData::Log(log_details) = monitor_match.match_data {
            assert_eq!(log_details.block_number, 456);
            assert_eq!(log_details.transaction_hash, TxHash::default());
            assert_eq!(log_details.contract_address, Address::default());
            assert_eq!(log_details.log_index, 15);
            assert_eq!(log_details.details.name, "TestLog");
            assert_eq!(
                log_details.details.params,
                serde_json::json!({"param1": "value1", "param2": 42})
            );
        } else {
            panic!("Expected MatchData::Log variant");
        }
    }
}
