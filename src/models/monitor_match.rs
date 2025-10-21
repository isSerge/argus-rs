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

    /// Name of the monitor that found the match.
    pub monitor_name: String,

    /// Name of the action that will handle notifications for this match.
    pub action_name: String,

    /// The block number where the match was found.
    pub block_number: u64,

    /// The transaction hash associated with the match.
    pub transaction_hash: TxHash,

    /// The data associated with the match.
    #[serde(flatten)]
    pub match_data: MatchData,
    /// The decoded call data, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decoded_call: Option<Value>,
}

impl MonitorMatch {
    /// Creates a new `MonitorMatchBuilder` to construct a `MonitorMatch`.
    pub fn builder(
        monitor_id: i64,
        monitor_name: String,
        action_name: String,
        block_number: u64,
        transaction_hash: TxHash,
    ) -> MonitorMatchBuilder {
        MonitorMatchBuilder {
            monitor_id,
            monitor_name,
            action_name,
            block_number,
            transaction_hash,
            match_data: None,
            decoded_call: None,
        }
    }
}

/// A builder for creating `MonitorMatch` instances.
pub struct MonitorMatchBuilder {
    monitor_id: i64,
    monitor_name: String,
    action_name: String,
    block_number: u64,
    transaction_hash: TxHash,
    match_data: Option<MatchData>,
    decoded_call: Option<Value>,
}

impl MonitorMatchBuilder {
    /// Sets the match data to a transaction match.
    pub fn transaction_match(mut self, details: Value) -> Self {
        self.match_data = Some(MatchData::Transaction { details });
        self
    }

    /// Sets the match data to a log match.
    pub fn log_match(mut self, log_details: LogDetails, tx_details: Value) -> Self {
        self.match_data = Some(MatchData::Log { log_details, tx_details });
        self
    }

    /// Sets the decoded call data.
    pub fn decoded_call(mut self, decoded_call: Option<Value>) -> Self {
        self.decoded_call = decoded_call;
        self
    }

    /// Builds the `MonitorMatch` instance.
    pub fn build(self) -> MonitorMatch {
        MonitorMatch {
            monitor_id: self.monitor_id,
            monitor_name: self.monitor_name,
            action_name: self.action_name,
            block_number: self.block_number,
            transaction_hash: self.transaction_hash,
            match_data: self.match_data.expect(
                "Match data must be set via transaction_match() or log_match() before calling \
                 build()",
            ),
            decoded_call: self.decoded_call,
        }
    }
}

/// Represents the data associated with a monitor match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MatchData {
    /// A log/event match.
    Log {
        /// Details about the log/event.
        #[serde(rename = "log")]
        log_details: LogDetails,

        /// Details about the transaction that included the log.
        #[serde(rename = "tx")]
        tx_details: Value,
    },
    /// A transaction match.
    Transaction {
        /// Details about the transaction.
        #[serde(rename = "tx")]
        details: Value,
    },
}

/// Core details about a log/event match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogDetails {
    /// The address of the contract that emitted the event.
    pub address: Address,
    /// The log index within the block.
    pub log_index: u64,
    /// The name of the log/event.
    pub name: String,
    /// The decoded parameters of the log/event.
    pub params: Value,
}

#[cfg(test)]
mod tests {
    use alloy::primitives::address;
    use serde_json::json;

    use super::*;

    #[test]
    fn test_monitor_match_construction_from_tx() {
        let monitor_match = MonitorMatch::builder(
            1,
            "Test Monitor".to_string(),
            "Test Action".to_string(),
            123,
            TxHash::default(),
        )
        .transaction_match(json!({"key": "value"}))
        .decoded_call(None)
        .build();

        assert_eq!(monitor_match.monitor_id, 1);
        assert_eq!(monitor_match.monitor_name, "Test Monitor");
        assert_eq!(monitor_match.action_name, "Test Action");
        assert_eq!(monitor_match.block_number, 123);
        assert_eq!(monitor_match.transaction_hash, TxHash::default());
        assert!(monitor_match.decoded_call.is_none());
        match monitor_match.match_data {
            MatchData::Transaction { details } => {
                assert_eq!(details, json!({"key": "value"}));
            }
            _ => panic!("Expected MatchData::Transaction variant"),
        }
    }

    #[test]
    fn test_monitor_match_construction_from_log() {
        let monitor_name = "Log Monitor".to_string();
        let action_name = "Log Action".to_string();
        let monitor_id = 1;
        let block_number = 456;

        let log_details = LogDetails {
            address: Address::default(),
            log_index: 15,
            name: "TestLog".to_string(),
            params: json!({"param1": "value1"}),
        };
        let tx_details = json!({"from": "0x123"});

        let monitor_match = MonitorMatch::builder(
            monitor_id,
            monitor_name.clone(),
            action_name.clone(),
            block_number,
            TxHash::default(),
        )
        .log_match(log_details.clone(), tx_details.clone())
        .decoded_call(None)
        .build();

        assert_eq!(monitor_match.monitor_id, monitor_id);
        assert_eq!(monitor_match.monitor_name, monitor_name);
        assert_eq!(monitor_match.action_name, action_name);
        assert_eq!(monitor_match.block_number, block_number);

        match monitor_match.match_data {
            MatchData::Log { log_details, tx_details: tx } => {
                assert_eq!(log_details.name, "TestLog");
                assert_eq!(log_details.params, json!({"param1": "value1"}));
                assert_eq!(tx, tx_details);
            }
            _ => panic!("Expected MatchData::Log variant"),
        }
    }

    #[test]
    fn test_monitor_match_transaction_roundtrip() {
        // --- Case 1: Without decoded_call ---
        let match_no_call = MonitorMatch::builder(
            1,
            "Test Monitor".to_string(),
            "Test Action".to_string(),
            123,
            TxHash::default(),
        )
        .transaction_match(json!({ "from": "0x4976...", "value": "22545..." }))
        .decoded_call(None)
        .build();

        let expected_json_no_call = json!({
            "monitor_id": 1,
            "monitor_name": "Test Monitor",
            "action_name": "Test Action",
            "block_number": 123,
            "transaction_hash": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "tx": {
                "from": "0x4976...",
                "value": "22545..."
            }
        });

        let serialized = serde_json::to_value(&match_no_call).unwrap();
        assert_eq!(serialized, expected_json_no_call);
        let deserialized: MonitorMatch = serde_json::from_value(serialized).unwrap();
        assert_eq!(deserialized, match_no_call);

        // --- Case 2: With decoded_call ---
        let decoded_call_data = json!({
            "name": "transfer",
            "params": { "_to": "0x123...", "_value": "1000" }
        });
        let match_with_call = MonitorMatch::builder(
            1,
            "Test Monitor".to_string(),
            "Test Action".to_string(),
            123,
            TxHash::default(),
        )
        .transaction_match(json!({ "from": "0x4976...", "value": "22545..." }))
        .decoded_call(Some(decoded_call_data.clone()))
        .build();

        let expected_json_with_call = json!({
            "monitor_id": 1,
            "monitor_name": "Test Monitor",
            "action_name": "Test Action",
            "block_number": 123,
            "transaction_hash": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "tx": {
                "from": "0x4976...",
                "value": "22545..."
            },
            "decoded_call": decoded_call_data
        });

        let serialized = serde_json::to_value(&match_with_call).unwrap();
        assert_eq!(serialized, expected_json_with_call);
        let deserialized: MonitorMatch = serde_json::from_value(serialized).unwrap();
        assert_eq!(deserialized, match_with_call);
    }

    #[test]
    fn test_monitor_match_log_roundtrip() {
        let contract_address = address!("0x0000000000000000000000000000000000000011");
        let log_details = LogDetails {
            address: contract_address,
            log_index: 15,
            name: "Transfer".to_string(),
            params: json!({ "value": "22545..." }),
        };
        let tx_details = json!({ "from": "0x4976..." });

        let monitor_match = MonitorMatch::builder(
            2,
            "Log Monitor".to_string(),
            "Log Action".to_string(),
            456,
            TxHash::default(),
        )
        .log_match(log_details, tx_details)
        .decoded_call(None)
        .build();

        let expected_json = json!({
            "monitor_id": 2,
            "monitor_name": "Log Monitor",
            "action_name": "Log Action",
            "block_number": 456,
            "transaction_hash": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "tx": {
                "from": "0x4976..."
            },
            "log": {
                "address": contract_address.to_checksum(None),
                "log_index": 15,
                "name": "Transfer",
                "params": {
                    "value": "22545..."
                }
            }
        });

        let serialized = serde_json::to_value(&monitor_match).unwrap();
        assert_eq!(serialized, expected_json);
        let deserialized: MonitorMatch = serde_json::from_value(serialized).unwrap();
        assert_eq!(deserialized, monitor_match);
    }

    #[test]
    fn test_monitor_match_log_deserialization_missing_tx_field() {
        let contract_address_str = "0x0000000000000000000000000000000000000011";
        let contract_address = contract_address_str.parse::<Address>().unwrap();

        // Simulate a JSON string where 'tx' is missing for a log-based match
        let json_str = format!(
            r#"{{
            "monitor_id": 2,
            "monitor_name": "Log Monitor",
            "action_name": "Log Action",
            "block_number": 456,
            "transaction_hash": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "log": {{
                "address": "{}",
                "log_index": 15,
                "name": "Transfer",
                "params": {{
                    "value": "22545..."
                }}
            }}
        }}"#,
            contract_address.to_checksum(None)
        );

        let deserialized: Result<MonitorMatch, _> = serde_json::from_str(&json_str);

        // Expect deserialization to fail due to missing 'tx' field
        assert!(deserialized.is_err());
        let error_message = deserialized.unwrap_err().to_string();
        assert!(
            error_message.contains("data did not match any variant of untagged enum MatchData")
        );
    }
}
