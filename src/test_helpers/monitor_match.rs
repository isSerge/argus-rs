use alloy::primitives::{Address, TxHash};
use serde_json::json;

use crate::models::monitor_match::{LogDetails, LogMatchData, MatchData, MonitorMatch};

/// Creates a sample `MonitorMatch` for testing purposes.
pub fn create_monitor_match(notifier_name: String) -> MonitorMatch {
    MonitorMatch {
        monitor_id: 0,
        monitor_name: "Test Monitor".to_string(),
        notifier_name,
        block_number: 123,
        transaction_hash: TxHash::default(),
        match_data: MatchData::Log(LogMatchData {
            log_details: LogDetails {
                address: Address::default(),
                log_index: 0,
                name: "Test Log".to_string(),
                params: json!({
                    "param1": "value1",
                    "param2": 42,
                }),
            },
            tx_details: json!({}), // Default empty transaction details for test
        }),
    }
}
