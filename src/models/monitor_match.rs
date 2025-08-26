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

    /// The block number where the match was found.
    pub block_number: u64,

    /// The transaction hash associated with the match.
    pub transaction_hash: TxHash,

    /// The data associated with the match, which can vary based on the type
    /// (e.g., transaction details, log details).
    #[serde(flatten)]
    pub match_data: MatchData,
}

impl MonitorMatch {
    /// Creates a new `MonitorMatch` instance for a transaction-based match.
    ///
    /// This constructor populates the `match_data` field with
    /// `MatchData::Transaction`, including additional transaction details.
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
            block_number,
            transaction_hash,
            match_data: MatchData::Transaction(TransactionDetails { details }),
        }
    }

    /// Creates a new `MonitorMatch` instance for a log-based match.
    ///
    /// This constructor populates the `match_data` field with `MatchData::Log`,
    /// including contract address, log index, and decoded log details (name and
    /// parameters).
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
            block_number,
            transaction_hash,
            match_data: MatchData::Log(LogDetails {
                contract_address,
                log_index,
                log_name,
                log_params,
            }),
        }
    }
}

/// Represents the data associated with a monitor match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum MatchData {
    /// A transaction match.
    Transaction(TransactionDetails),
    /// A log/event match.
    Log(LogDetails),
}

/// Details about a transaction match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionDetails {
    /// Additional details about the transaction, flattened into this struct.
    #[serde(flatten)]
    pub details: serde_json::Value,
}

/// Details about a log/event match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogDetails {
    /// The address of the contract that was the main subject of the monitor.
    pub contract_address: Address,

    /// The log index within the block, if the trigger was an event.
    pub log_index: u64,

    /// The name of the log/event.
    pub log_name: String,

    /// The parameters associated with the log/event, flattened into this
    /// struct.
    #[serde(flatten)]
    pub log_params: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use alloy::primitives::address;

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

        assert_eq!(monitor_match.block_number, 123);
        assert_eq!(monitor_match.transaction_hash, TxHash::default());
        if let MatchData::Transaction(tx_details) = monitor_match.match_data {
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

        assert_eq!(monitor_match.block_number, 456);
        assert_eq!(monitor_match.transaction_hash, TxHash::default());
        if let MatchData::Log(log_details) = monitor_match.match_data {
            assert_eq!(log_details.contract_address, Address::default());
            assert_eq!(log_details.log_index, 15);
            assert_eq!(log_details.log_name, "TestLog");
            assert_eq!(log_details.log_params, serde_json::json!({"param1": "value1", "param2": 42}));
        } else {
            panic!("Expected MatchData::Log variant");
        }
    }

    #[test]
    fn test_monitor_match_tx_json_serialization() {
        let monitor_match = MonitorMatch::new_tx_match(
            1,
            "Test Monitor".to_string(),
            "Test Notifier".to_string(),
            123,
            TxHash::default(),
            serde_json::json!({
                "from": "0x4976...",
                "to": "0xA76B...",
                "value": "22545...",
                "gas_used": "18000",
                "status": 1
            }),
        );

        let expected_json = serde_json::json!({
            "monitor_id": 1,
            "monitor_name": "Test Monitor",
            "notifier_name": "Test Notifier",
            "block_number": 123,
            "transaction_hash": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "type": "transaction",
            "from": "0x4976...",
            "to": "0xA76B...",
            "value": "22545...",
            "gas_used": "18000",
            "status": 1
        });

        let serialized = serde_json::to_value(&monitor_match).unwrap();
        assert_eq!(serialized, expected_json);
    }

    #[test]
    fn test_monitor_match_log_json_serialization() {
        let contract_address = address!("0x0000000000000000000000000000000000000011");
        let monitor_match = MonitorMatch::new_log_match(
            2,
            "Log Monitor".to_string(),
            "Log Notifier".to_string(),
            456,
            TxHash::default(),
            contract_address,
            15,
            "Transfer".to_string(),
            serde_json::json!({
                "from": "0x4976...",
                "to": "0xA76B...",
                "value": "22545..."
            }),
        );

        let expected_json = serde_json::json!({
            "monitor_id": 2,
            "monitor_name": "Log Monitor",
            "notifier_name": "Log Notifier",
            "block_number": 456,
            "transaction_hash": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "type": "log",
            "contract_address": contract_address.to_checksum(None),
            "log_index": 15,
            "log_name": "Transfer",
            "from": "0x4976...",
            "to": "0xA76B...",
            "value": "22545..."
        });

        let serialized = serde_json::to_value(&monitor_match).unwrap();
        assert_eq!(serialized, expected_json);
    }
}
