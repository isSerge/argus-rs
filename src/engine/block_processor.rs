//! This module provides functions for processing and correlating raw blockchain
//! data.
//!
//! The primary function, `process_blocks_batch`, takes raw `BlockData` and
//! transforms it into `CorrelatedBlockData`, which groups transactions with
//! their corresponding logs and receipts. This is the preparatory step before
//! the data is passed to the `FilteringEngine`.

use alloy::rpc::types::BlockTransactions;

use crate::models::{
    BlockData, CorrelatedBlockData, CorrelatedBlockItem, transaction::Transaction,
};

/// Correlates a single block's data, grouping transactions with their logs and
/// receipts. This function is the core of the processing logic.
fn process_block(block_data: BlockData) -> CorrelatedBlockData {
    tracing::debug!(block_number = block_data.block.header.number, "Correlating block data.");

    let mut correlated_items = Vec::new();

    if let BlockTransactions::Full(transactions) = block_data.block.transactions {
        correlated_items.reserve(transactions.len());
        for tx in transactions {
            let tx: Transaction = tx.into();
            let tx_hash = tx.hash();

            let logs = block_data.logs.get(&tx_hash).cloned().unwrap_or_default();
            let receipt = block_data.receipts.get(&tx_hash).cloned();

            let correlated_item = CorrelatedBlockItem::new(tx, logs, receipt);
            correlated_items.push(correlated_item);
        }
    } else {
        tracing::warn!(
            block_number = block_data.block.header.number,
            "Block does not contain full transaction objects. Skipping correlation."
        );
    }

    CorrelatedBlockData { block_number: block_data.block.header.number, items: correlated_items }
}

/// Processes a batch of `BlockData` into `CorrelatedBlockData` for the
/// filtering engine.
///
/// This is the main public entry point for this module.
pub async fn process_blocks_batch(
    blocks: Vec<BlockData>,
) -> Result<Vec<CorrelatedBlockData>, Box<dyn std::error::Error + Send + Sync>> {
    if blocks.is_empty() {
        return Ok(Vec::new());
    }

    let count = blocks.len();
    tracing::debug!(
        block_count = count,
        first_block = blocks.first().map(|b| b.block.header.number),
        last_block = blocks.last().map(|b| b.block.header.number),
        "Processing batch of blocks."
    );

    let correlated_blocks: Vec<CorrelatedBlockData> =
        tokio::task::spawn_blocking(move || blocks.into_iter().map(process_block).collect())
            .await
            .unwrap();

    tracing::info!(total_correlated_blocks = count, "Batch correlation completed.");

    Ok(correlated_blocks)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use alloy::primitives::{B256, Bytes, U256, address, b256};

    use super::*;
    use crate::test_helpers::*;

    #[test]
    fn test_process_block_happy_path() {
        let block_number = 123;
        let tx_hash = b256!("1111111111111111111111111111111111111111111111111111111111111111");
        let contract_address = address!("0000000000000000000000000000000000000001");

        let tx = TransactionBuilder::new()
            .hash(tx_hash)
            .to(Some(contract_address))
            .block_number(block_number)
            .build();

        let log = LogBuilder::new()
            .address(contract_address)
            .data(Bytes::from(U256::from(1000).to_be_bytes::<32>()))
            .build();

        let block = BlockBuilder::new().number(block_number).transaction(tx.clone()).build();

        let mut logs_by_tx = HashMap::new();
        logs_by_tx.insert(tx_hash, vec![log.into()]);

        let block_data = BlockData::new(block, HashMap::new(), logs_by_tx);

        let correlated_block = process_block(block_data);

        assert_eq!(correlated_block.items.len(), 1);
        let correlated_item = &correlated_block.items[0];
        assert_eq!(correlated_item.transaction.hash(), tx_hash);
        assert_eq!(correlated_item.transaction.block_number(), Some(block_number));
        assert_eq!(correlated_item.logs.len(), 1);
    }

    #[test]
    fn test_process_block_no_full_transactions() {
        let block = BlockBuilder::new().build(); // Builds with BlockTransactions::Hashes by default
        let block_data = BlockData::new(block, HashMap::new(), HashMap::new());
        let correlated_block = process_block(block_data);
        assert!(correlated_block.items.is_empty());
    }

    #[tokio::test]
    async fn test_process_blocks_batch_multiple_blocks() {
        let contract_address = address!("0000000000000000000000000000000000000001");
        let mut blocks = Vec::new();

        for block_num in 100..103 {
            let tx_hash = B256::from([block_num as u8; 32]);
            let tx = TransactionBuilder::new().hash(tx_hash).to(Some(contract_address)).build();
            let log = LogBuilder::new().address(contract_address).build();
            let block = BlockBuilder::new().number(block_num).transaction(tx).build();
            let mut logs_by_tx = HashMap::new();
            logs_by_tx.insert(tx_hash, vec![log.into()]);
            blocks.push(BlockData::new(block, HashMap::new(), logs_by_tx));
        }

        let correlated_blocks = process_blocks_batch(blocks).await.unwrap();

        assert_eq!(correlated_blocks.len(), 3);
        for block in &correlated_blocks {
            assert_eq!(block.items.len(), 1);
        }
    }

    #[tokio::test]
    async fn test_process_blocks_batch_empty() {
        let correlated = process_blocks_batch(Vec::new()).await.unwrap();
        assert!(correlated.is_empty());
    }
}
