//! A set of helpers for testing

use crate::models::transaction::Transaction;
use alloy::{
    consensus::{ReceiptEnvelope, ReceiptWithBloom},
    primitives::{Address, B256, Bytes, U256},
    rpc::types::{Transaction as AlloyTransaction, TransactionReceipt},
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

/// A builder for creating `Transaction` instances for testing.
#[derive(Debug, Clone, Default)]
pub struct TransactionBuilder {
    to: Option<Address>,
    input: Bytes,
    from: Option<Address>,
    value: Option<U256>,
    nonce: Option<u64>,
    gas_limit: Option<u64>,
}

impl TransactionBuilder {
    /// Creates a new `TransactionBuilder`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the `to` address for the transaction.
    pub fn to(mut self, to: Address) -> Self {
        self.to = Some(to);
        self
    }

    /// Sets the `input` data for the transaction.
    pub fn input(mut self, input: Bytes) -> Self {
        self.input = input;
        self
    }

    /// Sets the `from` address for the transaction.
    pub fn from(mut self, from: Address) -> Self {
        self.from = Some(from);
        self
    }

    /// Sets the `value` for the transaction.
    pub fn value(mut self, value: U256) -> Self {
        self.value = Some(value);
        self
    }

    /// Sets the `nonce` for the transaction.
    pub fn nonce(mut self, nonce: u64) -> Self {
        self.nonce = Some(nonce);
        self
    }

    /// Sets the `gas_limit` for the transaction.
    pub fn gas_limit(mut self, gas_limit: u64) -> Self {
        self.gas_limit = Some(gas_limit);
        self
    }

    /// Builds the `Transaction` with default values.
    pub fn build(self) -> Transaction {
        let from = self.from.unwrap_or_default();
        let value = self.value.unwrap_or(U256::ZERO);
        self.build_from_parts(from, value)
    }

    /// Builds the `Transaction` with specified from address and value.
    pub fn build_from_parts(self, from: Address, value: U256) -> Transaction {
        // Use builder values or fallback to provided/default values
        let actual_from = self.from.unwrap_or(from);
        let actual_value = self.value.unwrap_or(value);
        let nonce = self.nonce.unwrap_or(0);
        let gas_limit = self.gas_limit.unwrap_or(21000);
        
        // Create a JSON representation of a transaction that we can deserialize
        let to_field = match self.to {
            Some(addr) => format!("\"0x{:x}\"", addr),
            None => "null".to_string(),
        };
        
        let input_hex = format!("0x{}", hex::encode(&self.input));
        let value_hex = format!("0x{:x}", actual_value);
        let from_hex = format!("0x{:x}", actual_from);
        let nonce_hex = format!("0x{:x}", nonce);
        let gas_hex = format!("0x{:x}", gas_limit);
        
        let tx_json = format!(
            r#"{{
                "blockHash": "0x8e38b4dbf6b11fcc3b9dee84fb7986e29ca0a02cecd8977c161ff7333329681e",
                "blockNumber": "0x1",
                "hash": "0xe9e91f1ee4b56c0df2e9f06c2b8c27c6076195a88a7b8537ba8313d80e6f124e",
                "transactionIndex": "0x0",
                "type": "0x2",
                "nonce": "{}",
                "input": "{}",
                "maxFeePerGas": "0x77359400",
                "maxPriorityFeePerGas": "0x3b9aca00",
                "chainId": "0x1",
                "accessList": [],
                "gas": "{}",
                "from": "{}",
                "to": {},
                "value": "{}",
                "r": "0x1",
                "s": "0x1",
                "v": "0x1"
            }}"#,
            nonce_hex, input_hex, gas_hex, from_hex, to_field, value_hex
        );
        
        let alloy_tx: AlloyTransaction = serde_json::from_str(&tx_json)
            .expect("Failed to create transaction from JSON");
            
        Transaction(alloy_tx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::address;

    #[test]
    fn test_transaction_builder() {
        let contract_addr = address!("1234567890123456789012345678901234567890");
        let from_addr = address!("abcdefabcdefabcdefabcdefabcdefabcdefabcd");
        let input_data = Bytes::from(vec![0xa9, 0x05, 0x9c, 0xbb]); // transfer function selector
        
        let tx = TransactionBuilder::new()
            .to(contract_addr)
            .from(from_addr)
            .input(input_data.clone())
            .value(U256::from(1000))
            .nonce(42)
            .gas_limit(50000)
            .build();
        
        // Test that the transaction was built correctly
        assert_eq!(tx.to(), Some(contract_addr));
        assert_eq!(tx.from(), from_addr);
        assert_eq!(tx.input(), &input_data);
        assert_eq!(tx.value(), U256::from(1000));
        assert_eq!(tx.nonce(), 42);
        assert_eq!(tx.gas(), 50000);
    }

    #[test]
    fn test_transaction_builder_minimal() {
        let tx = TransactionBuilder::new().build();
        
        // Test defaults
        assert_eq!(tx.to(), None); // Contract creation
        assert_eq!(tx.from(), Address::default());
        assert_eq!(tx.value(), U256::ZERO);
        assert_eq!(tx.nonce(), 0);
        assert_eq!(tx.gas(), 21000);
        assert!(tx.input().is_empty());
    }
}
