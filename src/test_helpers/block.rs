//! A builder for creating `Block` instances for testing.

use alloy::{
    primitives::B256,
    rpc::types::{Block, BlockTransactions, Header},
};

use crate::models::transaction::Transaction;

/// A builder for creating `Block` instances for testing.
#[derive(Debug, Clone, Default)]
pub struct BlockBuilder {
    header: Header,
    transactions: Vec<Transaction>,
}

impl BlockBuilder {
    /// Creates a new `BlockBuilder`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the block number.
    pub fn number(mut self, number: u64) -> Self {
        self.header.number = number;
        self
    }

    /// Sets the block hash.
    pub fn hash(mut self, hash: B256) -> Self {
        self.header.hash = hash;
        self
    }

    /// Adds a transaction to the block.
    pub fn transaction(mut self, tx: Transaction) -> Self {
        self.transactions.push(tx);
        self
    }

    /// Builds the `Block` with the provided values.
    pub fn build(self) -> Block {
        let txs = self.transactions.into_iter().map(|tx| tx.0).collect();
        Block {
            header: self.header,
            transactions: BlockTransactions::Full(txs),
            uncles: Default::default(),
            withdrawals: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::{address, b256};

    use super::*;
    use crate::test_helpers::TransactionBuilder;

    #[test]
    fn test_block_builder() {
        let tx_hash = b256!("1111111111111111111111111111111111111111111111111111111111111111");
        let tx = TransactionBuilder::new()
            .hash(tx_hash)
            .to(Some(address!("0000000000000000000000000000000000000001")))
            .build();

        let block =
            BlockBuilder::new().number(123).hash(B256::repeat_byte(0x42)).transaction(tx).build();

        assert_eq!(block.header.number, 123);
        assert_eq!(block.header.hash, B256::repeat_byte(0x42));
        assert!(matches!(block.transactions, BlockTransactions::Full(_)));

        if let BlockTransactions::Full(txs) = block.transactions {
            assert_eq!(txs.len(), 1);
            assert_eq!(*txs[0].inner.hash(), tx_hash);
        }
    }
}
