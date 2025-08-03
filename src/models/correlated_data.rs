//! This module defines data structures for correlated blockchain data.

use alloy::{
    primitives::TxHash,
    rpc::types::TransactionReceipt,
};
use crate::abi::DecodedLog;
use crate::models::transaction::Transaction;

/// Represents a correlated set of data for a single transaction within a block.
/// This is the unit of data that the `FilteringEngine` will evaluate.
#[derive(Debug, Clone)]
pub struct CorrelatedBlockItem<'a> {
    /// The transaction itself.
    pub transaction: &'a Transaction,
    /// All logs associated with this transaction, including decoded versions.
    pub decoded_logs: Vec<DecodedLog<'a>>,
    /// The transaction receipt, if available and needed.
    pub receipt: Option<&'a TransactionReceipt>,
    // Potentially other correlated data points like internal calls, state diffs, etc.
}

impl<'a> CorrelatedBlockItem<'a> {
    /// Creates a new `CorrelatedBlockItem`.
    pub fn new(
        transaction: &'a Transaction,
        decoded_logs: Vec<DecodedLog<'a>>,
        receipt: Option<&'a TransactionReceipt>,
    ) -> Self {
        Self {
            transaction,
            decoded_logs,
            receipt,
        }
    }

    /// Returns the hash of the transaction.
    pub fn tx_hash(&self) -> TxHash {
        self.transaction.hash()
    }
}
