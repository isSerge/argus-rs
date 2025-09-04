//! This module defines the `MonitorMatch` struct.

use std::fmt;

use alloy::primitives::{Address, TxHash};
use serde::{
    Deserialize, Serialize,
    de::{self, Deserializer, MapAccess, Visitor},
    ser::{SerializeStruct, Serializer},
};
use serde_json::Value;

/// Represents a match found by a monitor, containing detailed information
/// about the trigger.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub match_data: MatchData,
}

impl MonitorMatch {
    /// Creates a new `MonitorMatch` instance for a transaction-based match.
    pub fn new_tx_match(
        monitor_id: i64,
        monitor_name: String,
        notifier_name: String,
        block_number: u64,
        transaction_hash: TxHash,
        details: Value,
    ) -> Self {
        Self {
            monitor_id,
            monitor_name,
            notifier_name,
            block_number,
            transaction_hash,
            match_data: MatchData::Transaction(TransactionMatchData { details }),
        }
    }

    /// Creates a new `MonitorMatch` instance for a log-based match.
    pub fn new_log_match(
        monitor_id: i64,
        monitor_name: String,
        notifier_name: String,
        block_number: u64,
        transaction_hash: TxHash,
        log_details: LogDetails,
        tx_details: Value,
    ) -> Self {
        Self {
            monitor_id,
            monitor_name,
            notifier_name,
            block_number,
            transaction_hash,
            match_data: MatchData::Log(LogMatchData { log_details, tx_details }),
        }
    }
}

/// Represents the data associated with a monitor match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchData {
    /// A transaction match.
    Transaction(TransactionMatchData),
    /// A log/event match.
    Log(LogMatchData),
}

/// Details for a transaction-based match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionMatchData {
    /// The JSON-serialized details of the transaction.
    pub details: Value,
}

/// Details for a log-based match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogMatchData {
    /// The details of the specific log that matched.
    pub log_details: LogDetails,
    /// The details of the parent transaction.
    pub tx_details: Value,
}

/// Core details about a log/event match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogDetails {
    /// The address of the contract that emitted the event.
    pub contract_address: Address,
    /// The log index within the block.
    pub log_index: u64,
    /// The name of the log/event.
    pub name: String,
    /// The decoded parameters of the log/event.
    pub params: Value,
}

// --- Custom Serialization and Deserialization for MonitorMatch ---

impl Serialize for MonitorMatch {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let field_count = match &self.match_data {
            MatchData::Transaction(_) => 6, // 5 base fields + "tx"
            MatchData::Log(_) => 7,         // 5 base fields + "tx" + "log"
        };

        let mut state = serializer.serialize_struct("MonitorMatch", field_count)?;
        state.serialize_field("monitor_id", &self.monitor_id)?;
        state.serialize_field("monitor_name", &self.monitor_name)?;
        state.serialize_field("notifier_name", &self.notifier_name)?;
        state.serialize_field("block_number", &self.block_number)?;
        state.serialize_field("transaction_hash", &self.transaction_hash)?;

        match &self.match_data {
            MatchData::Transaction(data) => {
                state.serialize_field("tx", &data.details)?;
            }
            MatchData::Log(data) => {
                state.serialize_field("tx", &data.tx_details)?;
                state.serialize_field("log", &data.log_details)?;
            }
        }

        state.end()
    }
}

impl<'de> Deserialize<'de> for MonitorMatch {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            MonitorId,
            MonitorName,
            NotifierName,
            BlockNumber,
            TransactionHash,
            Tx,
            Log,
        }

        struct MonitorMatchVisitor;

        impl<'de> Visitor<'de> for MonitorMatchVisitor {
            type Value = MonitorMatch;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct MonitorMatch")
            }

            fn visit_map<V>(self, mut map: V) -> Result<MonitorMatch, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut monitor_id = None;
                let mut monitor_name = None;
                let mut notifier_name = None;
                let mut block_number = None;
                let mut transaction_hash = None;
                let mut tx = None;
                let mut log = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::MonitorId => {
                            if monitor_id.is_some() {
                                return Err(de::Error::duplicate_field("monitor_id"));
                            }
                            monitor_id = Some(map.next_value()?);
                        }
                        Field::MonitorName => {
                            if monitor_name.is_some() {
                                return Err(de::Error::duplicate_field("monitor_name"));
                            }
                            monitor_name = Some(map.next_value()?);
                        }
                        Field::NotifierName => {
                            if notifier_name.is_some() {
                                return Err(de::Error::duplicate_field("notifier_name"));
                            }
                            notifier_name = Some(map.next_value()?);
                        }
                        Field::BlockNumber => {
                            if block_number.is_some() {
                                return Err(de::Error::duplicate_field("block_number"));
                            }
                            block_number = Some(map.next_value()?);
                        }
                        Field::TransactionHash => {
                            if transaction_hash.is_some() {
                                return Err(de::Error::duplicate_field("transaction_hash"));
                            }
                            transaction_hash = Some(map.next_value()?);
                        }
                        Field::Tx => {
                            if tx.is_some() {
                                return Err(de::Error::duplicate_field("tx"));
                            }
                            tx = Some(map.next_value()?);
                        }
                        Field::Log => {
                            if log.is_some() {
                                return Err(de::Error::duplicate_field("log"));
                            }
                            log = Some(map.next_value()?);
                        }
                    }
                }

                let monitor_id =
                    monitor_id.ok_or_else(|| de::Error::missing_field("monitor_id"))?;
                let monitor_name =
                    monitor_name.ok_or_else(|| de::Error::missing_field("monitor_name"))?;
                let notifier_name =
                    notifier_name.ok_or_else(|| de::Error::missing_field("notifier_name"))?;
                let block_number =
                    block_number.ok_or_else(|| de::Error::missing_field("block_number"))?;
                let transaction_hash =
                    transaction_hash.ok_or_else(|| de::Error::missing_field("transaction_hash"))?;
                let tx_details = tx.ok_or_else(|| de::Error::missing_field("tx"))?;

                let match_data = if let Some(log_details) = log {
                    MatchData::Log(LogMatchData { log_details, tx_details })
                } else {
                    MatchData::Transaction(TransactionMatchData { details: tx_details })
                };

                Ok(MonitorMatch {
                    monitor_id,
                    monitor_name,
                    notifier_name,
                    block_number,
                    transaction_hash,
                    match_data,
                })
            }
        }

        const FIELDS: &[&str] = &[
            "monitor_id",
            "monitor_name",
            "notifier_name",
            "block_number",
            "transaction_hash",
            "tx",
            "log",
        ];
        deserializer.deserialize_struct("MonitorMatch", FIELDS, MonitorMatchVisitor)
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::address;
    use serde_json::json;

    use super::*;

    #[test]
    fn test_monitor_match_construction_from_tx() {
        let monitor_match = MonitorMatch::new_tx_match(
            1,
            "Test Monitor".to_string(),
            "Test Notifier".to_string(),
            123,
            TxHash::default(),
            json!({"key": "value"}),
        );

        assert_eq!(monitor_match.monitor_id, 1);
        assert_eq!(monitor_match.monitor_name, "Test Monitor");
        assert_eq!(monitor_match.notifier_name, "Test Notifier");
        assert_eq!(monitor_match.block_number, 123);
        assert_eq!(monitor_match.transaction_hash, TxHash::default());

        match monitor_match.match_data {
            MatchData::Transaction(data) => {
                assert_eq!(data.details, json!({"key": "value"}));
            }
            _ => panic!("Expected MatchData::Transaction variant"),
        }
    }

    #[test]
    fn test_monitor_match_construction_from_log() {
        let log_details = LogDetails {
            contract_address: Address::default(),
            log_index: 15,
            name: "TestLog".to_string(),
            params: json!({"param1": "value1"}),
        };
        let tx_details = json!({"from": "0x123"});

        let monitor_match = MonitorMatch::new_log_match(
            2,
            "Log Monitor".to_string(),
            "Log Notifier".to_string(),
            456,
            TxHash::default(),
            log_details,
            tx_details.clone(),
        );

        assert_eq!(monitor_match.monitor_id, 2);
        assert_eq!(monitor_match.monitor_name, "Log Monitor");
        assert_eq!(monitor_match.notifier_name, "Log Notifier");
        assert_eq!(monitor_match.block_number, 456);

        match monitor_match.match_data {
            MatchData::Log(data) => {
                assert_eq!(data.log_details.name, "TestLog");
                assert_eq!(data.log_details.params, json!({"param1": "value1"}));
                assert_eq!(data.tx_details, tx_details);
            }
            _ => panic!("Expected MatchData::Log variant"),
        }
    }

    #[test]
    fn test_monitor_match_tx_serialization() {
        let monitor_match = MonitorMatch::new_tx_match(
            1,
            "Test Monitor".to_string(),
            "Test Notifier".to_string(),
            123,
            TxHash::default(),
            json!({ "from": "0x4976...", "value": "22545..." }),
        );

        let expected_json = json!({
            "monitor_id": 1,
            "monitor_name": "Test Monitor",
            "notifier_name": "Test Notifier",
            "block_number": 123,
            "transaction_hash": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "tx": {
                "from": "0x4976...",
                "value": "22545..."
            }
        });

        let serialized = serde_json::to_value(&monitor_match).unwrap();
        assert_eq!(serialized, expected_json);
    }

    #[test]
    fn test_monitor_match_log_serialization() {
        let contract_address = address!("0x0000000000000000000000000000000000000011");
        let log_details = LogDetails {
            contract_address,
            log_index: 15,
            name: "Transfer".to_string(),
            params: json!({ "value": "22545..." }),
        };
        let tx_details = json!({ "from": "0x4976..." });

        let monitor_match = MonitorMatch::new_log_match(
            2,
            "Log Monitor".to_string(),
            "Log Notifier".to_string(),
            456,
            TxHash::default(),
            log_details,
            tx_details,
        );

        let expected_json = json!({
            "monitor_id": 2,
            "monitor_name": "Log Monitor",
            "notifier_name": "Log Notifier",
            "block_number": 456,
            "transaction_hash": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "tx": {
                "from": "0x4976..."
            },
            "log": {
                "contract_address": contract_address.to_checksum(None),
                "log_index": 15,
                "name": "Transfer",
                "params": {
                    "value": "22545..."
                }
            }
        });

        let serialized = serde_json::to_value(&monitor_match).unwrap();
        assert_eq!(serialized, expected_json);
    }

    #[test]
    fn test_monitor_match_tx_deserialization() {
        let json_str = r#"{
            "monitor_id": 1,
            "monitor_name": "Test Monitor",
            "notifier_name": "Test Notifier",
            "block_number": 123,
            "transaction_hash": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "tx": {
                "from": "0x4976...",
                "value": "22545..."
            }
        }"#;

        let deserialized: MonitorMatch = serde_json::from_str(json_str).unwrap();
        let expected_match_data = MatchData::Transaction(TransactionMatchData {
            details: json!({ "from": "0x4976...", "value": "22545..." }),
        });

        assert_eq!(deserialized.monitor_id, 1);
        assert_eq!(deserialized.match_data, expected_match_data);
    }

    #[test]
    fn test_monitor_match_log_deserialization() {
        let contract_address_str = "0x0000000000000000000000000000000000000011";
        let contract_address = contract_address_str.parse::<Address>().unwrap();

        let json_str = format!(
            r#"{{
            "monitor_id": 2,
            "monitor_name": "Log Monitor",
            "notifier_name": "Log Notifier",
            "block_number": 456,
            "transaction_hash": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "tx": {{
                "from": "0x4976..."
            }},
            "log": {{
                "contract_address": "{}",
                "log_index": 15,
                "name": "Transfer",
                "params": {{
                    "value": "22545..."
                }}
            }}
        }}"#,
            contract_address.to_checksum(None)
        );

        let deserialized: MonitorMatch = serde_json::from_str(&json_str).unwrap();

        let expected_log_details = LogDetails {
            contract_address,
            log_index: 15,
            name: "Transfer".to_string(),
            params: json!({ "value": "22545..." }),
        };
        let expected_tx_details = json!({ "from": "0x4976..." });
        let expected_match_data = MatchData::Log(LogMatchData {
            log_details: expected_log_details,
            tx_details: expected_tx_details,
        });

        assert_eq!(deserialized.monitor_id, 2);
        assert_eq!(deserialized.match_data, expected_match_data);
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
            "notifier_name": "Log Notifier",
            "block_number": 456,
            "transaction_hash": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "log": {{
                "contract_address": "{}",
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
        assert!(error_message.contains("missing field `tx`"));
    }
}
