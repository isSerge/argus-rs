use alloy::primitives::TxHash;
use serde_json::json;

use crate::models::monitor_match::MonitorMatch;

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

/// Creates a test `MonitorMatch` for a transaction.
pub fn create_test_tx_monitor_match_with_hash(
    monitor_name: &str,
    action_name: &str,
    tx_hash: TxHash,
) -> MonitorMatch {
    MonitorMatch::builder(1, monitor_name.to_string(), action_name.to_string(), 123, tx_hash)
        .transaction_match(json!({"value": "100"}))
        .decoded_call(None)
        .build()
}
