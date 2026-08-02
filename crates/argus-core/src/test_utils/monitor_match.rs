use std::sync::atomic::{AtomicU64, Ordering};

use alloy::primitives::TxHash;
use serde_json::json;

use crate::models::monitor_match::MonitorMatch;

/// Atomic counter to generate unique transaction hashes for test monitor
/// matches.
static MATCH_TX_NONCE: AtomicU64 = AtomicU64::new(1);

fn unique_tx_hash() -> TxHash {
    let n = MATCH_TX_NONCE.fetch_add(1, Ordering::Relaxed);
    let mut bytes = [0u8; 32];
    bytes[24..].copy_from_slice(&n.to_be_bytes());
    TxHash::from(bytes)
}

/// Quick helper function for the most common case: transaction match with
/// defaults. Each call yields a distinct transaction hash so that batches of
/// generated matches remain individually dedupable.
pub fn create_test_monitor_match(monitor_name: &str, action_name: &str) -> MonitorMatch {
    MonitorMatch::builder(
        1,
        monitor_name.to_string(),
        action_name.to_string(),
        123,
        unique_tx_hash(),
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
