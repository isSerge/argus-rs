//! A set of helpers for testing

use alloy::{
    consensus::{ReceiptEnvelope, ReceiptWithBloom},
    primitives::{Address, B256},
    rpc::types::TransactionReceipt,
};

/// A builder for creating `TransactionReceipt` instances for testing.
#[derive(Debug, Default, Clone)]
pub struct ReceiptBuilder {
    transaction_hash: Option<B256>,
}

impl ReceiptBuilder {
    /// Creates a new `ReceiptBuilder`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the transaction hash for the receipt.
    pub fn transaction_hash(mut self, hash: B256) -> Self {
        self.transaction_hash = Some(hash);
        self
    }

    /// Builds the `TransactionReceipt` with the provided or default values.
    pub fn build(self) -> TransactionReceipt {
        TransactionReceipt {
            transaction_hash: self.transaction_hash.unwrap_or_default(),
            block_number: Some(123),
            transaction_index: Some(1),
            block_hash: Some(B256::default()),
            from: Address::default(),
            to: Some(Address::default()),
            gas_used: 21_000,
            contract_address: None,
            effective_gas_price: 1_000_000_000, // 1 Gwei
            blob_gas_used: None,
            blob_gas_price: None,
            inner: ReceiptEnvelope::Eip7702(ReceiptWithBloom::default()),
        }
    }
}
