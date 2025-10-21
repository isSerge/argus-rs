//! Helper functions for creating MonitorMatch instances in tests

use alloy::primitives::TxHash;
use serde_json::{Value, json};

use crate::models::monitor_match::{LogDetails, MonitorMatch};

/// Quick helper function for the most common case: transaction match with
/// defaults
pub fn create_test_monitor_match(monitor_name: &str, action_name: &str) -> MonitorMatch {
    MonitorMatch::builder(
        1,
        monitor_name.to_string(),
        action_name.to_string(),
        123,
        TxHash::default(),
    )
    .transaction_match(json!({"value": "100"}))
    .decoded_call(None)
    .build()
}

/// Quick helper function for transaction match with custom details
pub fn create_test_tx_monitor_match(
    monitor_name: &str,
    action_name: &str,
    tx_details: Value,
) -> MonitorMatch {
    MonitorMatch::builder(
        1,
        monitor_name.to_string(),
        action_name.to_string(),
        123,
        TxHash::default(),
    )
    .transaction_match(tx_details)
    .decoded_call(None)
    .build()
}

/// Quick helper function for log match
pub fn create_test_log_monitor_match(
    monitor_name: &str,
    action_name: &str,
    log_details: LogDetails,
    tx_details: Value,
) -> MonitorMatch {
    MonitorMatch::builder(
        1,
        monitor_name.to_string(),
        action_name.to_string(),
        123,
        TxHash::default(),
    )
    .log_match(log_details, tx_details)
    .decoded_call(None)
    .build()
}

/// Quick helper function with decoded call
pub fn create_test_monitor_match_with_call(
    monitor_name: &str,
    action_name: &str,
    function_name: &str,
    params: Value,
) -> MonitorMatch {
    MonitorMatch::builder(
        1,
        monitor_name.to_string(),
        action_name.to_string(),
        123,
        TxHash::default(),
    )
    .transaction_match(json!({"value": "100"}))
    .decoded_call(Some(json!({
        "name": function_name,
        "params": params
    })))
    .build()
}

/// Helper for creating a monitor match with custom ID and block number
pub fn create_test_monitor_match_custom(
    monitor_id: i64,
    monitor_name: &str,
    action_name: &str,
    block_number: u64,
    tx_hash: TxHash,
) -> MonitorMatch {
    MonitorMatch::builder(
        monitor_id,
        monitor_name.to_string(),
        action_name.to_string(),
        block_number,
        tx_hash,
    )
    .transaction_match(json!({"value": "100"}))
    .decoded_call(None)
    .build()
}
