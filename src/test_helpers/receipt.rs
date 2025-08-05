//! A builder for creating `TransactionReceipt` instances for testing.

use alloy::{
    consensus::{Eip658Value, Receipt, ReceiptEnvelope, ReceiptWithBloom},
    primitives::{Address, B256, Bloom},
    rpc::types::TransactionReceipt,
};

/// A builder for creating `TransactionReceipt` instances for testing.
#[derive(Debug, Default, Clone)]
pub struct ReceiptBuilder {
    transaction_hash: Option<B256>,
    block_number: Option<u64>,
    gas_used: Option<u64>,
    effective_gas_price: Option<u128>,
    status: Option<bool>,
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

    /// Sets the block number for the receipt.
    /// If not set, it defaults to `None`.
    pub fn block_number(mut self, number: u64) -> Self {
        self.block_number = Some(number);
        self
    }

    /// Sets the gas used for the receipt.
    pub fn gas_used(mut self, gas: u64) -> Self {
        self.gas_used = Some(gas);
        self
    }

    /// Sets the effective gas price for the receipt.
    pub fn effective_gas_price(mut self, price: u128) -> Self {
        self.effective_gas_price = Some(price);
        self
    }

    /// Sets the status for the receipt (true for success, false for failure).
    pub fn status(mut self, success: bool) -> Self {
        self.status = Some(success);
        self
    }

    /// Builds the `TransactionReceipt` with the provided or default values.
    pub fn build(self) -> TransactionReceipt {
        let status = if self.status.unwrap_or(true) {
            Eip658Value::Eip658(true)
        } else {
            Eip658Value::Eip658(false)
        };

        let inner_receipt = Receipt {
            status,
            cumulative_gas_used: self.gas_used.unwrap_or(21_000),
            logs: vec![],
        };

        let receipt_with_bloom = ReceiptWithBloom {
            receipt: inner_receipt,
            logs_bloom: Bloom::default(),
        };

        TransactionReceipt {
            transaction_hash: self.transaction_hash.unwrap_or_default(),
            block_number: self.block_number,
            transaction_index: Some(1),
            block_hash: Some(B256::default()),
            from: Address::default(),
            to: Some(Address::default()),
            gas_used: self.gas_used.unwrap_or(21_000),
            contract_address: None,
            effective_gas_price: self.effective_gas_price.unwrap_or(1_000_000_000), // 1 Gwei
            blob_gas_used: None,
            blob_gas_price: None,
            inner: ReceiptEnvelope::Eip1559(receipt_with_bloom),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::{address, b256};

    #[test]
    fn test_receipt_builder() {
        let receipt = ReceiptBuilder::new()
            .transaction_hash(b256!(
                "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
            ))
            .block_number(321)
            .gas_used(30_000)
            .effective_gas_price(2_000_000_000) // 2 Gwei
            .status(true)
            .build();

        assert_eq!(
            receipt.transaction_hash,
            b256!("0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef")
        );
        assert_eq!(receipt.block_number, Some(321));
        assert_eq!(receipt.transaction_index, Some(1));
        assert_eq!(
            receipt.from,
            address!("0x0000000000000000000000000000000000000000")
        );
        assert_eq!(
            receipt.to,
            Some(address!("0x0000000000000000000000000000000000000000"))
        );
        assert_eq!(receipt.gas_used, 30_000);
        assert_eq!(receipt.effective_gas_price, 2_000_000_000); // 2 Gwei
        assert!(receipt.contract_address.is_none());
        assert!(receipt.inner.status());
    }
}
