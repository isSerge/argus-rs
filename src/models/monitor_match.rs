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

    /// Name of the action that will handle notifications for this match.
    pub action_name: String,

    /// The block number where the match was found.
    pub block_number: u64,

    /// The transaction hash associated with the match.
    pub transaction_hash: TxHash,

    /// The data associated with the match.
    pub match_data: MatchData,
    /// The decoded call data, if available.
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
        self.match_data = Some(MatchData::Transaction(TransactionMatchData { details }));
        self
    }

    /// Sets the match data to a log match.
    pub fn log_match(mut self, log_details: LogDetails, tx_details: Value) -> Self {
        self.match_data = Some(MatchData::Log(LogMatchData { log_details, tx_details }));
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
    pub address: Address,
    /// The log index within the block.
    pub log_index: u64,
    /// The name of the log/event.
    pub name: String,
    /// The decoded parameters of the log/event.
    pub params: Value,
}

// Custom serialization and deserialization for MonitorMatch.
// Standard derive macros (#[derive(Serialize, Deserialize)]) cannot be used
// here because we need to flatten the fields from the nested `match_data` enum
// variants (TransactionMatchData or LogMatchData) directly into the parent
// MonitorMatch struct during serialization, rather than nesting them under a
// field. This ensures that all relevant match data appears at the top level of
// the serialized output.
impl Serialize for MonitorMatch {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // 5 base fields: monitor_id, monitor_name, action_name, block_number,
        // transaction_hash
        let base_fields = 5;
        let extra_fields = match &self.match_data {
            MatchData::Transaction(_) => 1, // "tx"
            MatchData::Log(_) => 2,         // "tx" + "log"
        };
        // Add 1 for decoded_call if it exists
        let decoded_call_field = if self.decoded_call.is_some() { 1 } else { 0 };
        let field_count = base_fields + extra_fields + decoded_call_field;

        let mut state = serializer.serialize_struct("MonitorMatch", field_count)?;
        state.serialize_field("monitor_id", &self.monitor_id)?;
        state.serialize_field("monitor_name", &self.monitor_name)?;
        state.serialize_field("action_name", &self.action_name)?;
        state.serialize_field("block_number", &self.block_number)?;
        state.serialize_field("transaction_hash", &self.transaction_hash)?;

        // Serialize decoded_call if present
        if let Some(ref decoded_call) = self.decoded_call {
            state.serialize_field("decoded_call", decoded_call)?;
        }

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
            ActionName,
            BlockNumber,
            TransactionHash,
            Tx,
            Log,
            DecodedCall,
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
                let mut action_name = None;
                let mut block_number = None;
                let mut transaction_hash = None;
                let mut tx = None;
                let mut log = None;
                let mut decoded_call = None;

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
                        Field::ActionName => {
                            if action_name.is_some() {
                                return Err(de::Error::duplicate_field("action_name"));
                            }
                            action_name = Some(map.next_value()?);
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
                        Field::DecodedCall => {
                            if decoded_call.is_some() {
                                return Err(de::Error::duplicate_field("decoded_call"));
                            }
                            decoded_call = Some(map.next_value()?);
                        }
                    }
                }

                let monitor_id =
                    monitor_id.ok_or_else(|| de::Error::missing_field("monitor_id"))?;
                let monitor_name =
                    monitor_name.ok_or_else(|| de::Error::missing_field("monitor_name"))?;
                let action_name =
                    action_name.ok_or_else(|| de::Error::missing_field("action_name"))?;
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
                    action_name,
                    block_number,
                    transaction_hash,
                    match_data,
                    decoded_call,
                })
            }
        }

        const FIELDS: &[&str] = &[
            "monitor_id",
            "monitor_name",
            "action_name",
            "block_number",
            "transaction_hash",
            "tx",
            "log",
            "decoded_call",
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
            MatchData::Transaction(data) => {
                assert_eq!(data.details, json!({"key": "value"}));
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
        let monitor_match = MonitorMatch::builder(
            1,
            "Test Monitor".to_string(),
            "Test Action".to_string(),
            123,
            TxHash::default(),
        )
        .transaction_match(json!({ "from": "0x4976...", "value": "22545..." }))
        .decoded_call(None)
        .build();

        let expected_json = json!({
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

        let serialized = serde_json::to_value(&monitor_match).unwrap();
        assert_eq!(serialized, expected_json);
    }

    #[test]
    fn test_monitor_match_log_serialization() {
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
    }

    #[test]
    fn test_monitor_match_tx_deserialization() {
        let json_str = r#"{
            "monitor_id": 1,
            "monitor_name": "Test Monitor",
            "action_name": "Test Action",
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
            "action_name": "Log Action",
            "block_number": 456,
            "transaction_hash": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "tx": {{
                "from": "0x4976..."
            }},
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

        let deserialized: MonitorMatch = serde_json::from_str(&json_str).unwrap();

        let expected_log_details = LogDetails {
            address: contract_address,
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
        assert!(error_message.contains("missing field `tx`"));
    }
}
