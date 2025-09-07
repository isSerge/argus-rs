//! This module defines the `BlockProcessor` component.
//!
//! The `BlockProcessor` is responsible for in-memory processing of blockchain
//! data, including log decoding, data correlation, and applying filtering
//! logic. It supports both single block processing and batch processing for
//! improved throughput.

use std::{result, sync::Arc};

use alloy::rpc::types::BlockTransactions;
use futures::future::join_all;
use thiserror::Error;

use crate::{
    abi::{AbiError, AbiService, DecodedLog},
    models::{BlockData, CorrelatedBlockItem, DecodedBlockData, transaction::Transaction},
    monitor::MonitorManager,
};

/// Custom error type for `BlockProcessor` operations.
#[derive(Error, Debug)]
pub enum BlockProcessorError {
    /// Wrapper for errors from the `AbiService`.
    #[error("ABI service error: {0}")]
    AbiService(#[from] crate::abi::AbiError),
}

/// The `BlockProcessor` is responsible for in-memory processing of `BlockData`.
/// This includes decoding logs and correlating data
pub struct BlockProcessor {
    abi_service: Arc<AbiService>,
    monitor_manager: Arc<MonitorManager>,
}

impl BlockProcessor {
    /// Creates a new `BlockProcessor` with the provided `AbiService`.
    pub fn new(abi_service: Arc<AbiService>, monitor_manager: Arc<MonitorManager>) -> Self {
        Self { abi_service, monitor_manager }
    }

    /// Processes the given `BlockData`, decodes logs, correlates data,
    /// and applies filtering logic.
    pub async fn process_block(
        &self,
        block_data: BlockData,
    ) -> Result<DecodedBlockData, BlockProcessorError> {
        tracing::debug!(block_number = block_data.block.header.number, "Processing block data.");

        // Load the current monitor snapshot
        let monitor_snapshot = self.monitor_manager.load();

        let num_txs = match &block_data.block.transactions {
            BlockTransactions::Full(txs) => txs.len(),
            _ => 0,
        };

        let mut decoded_block = DecodedBlockData {
            block_number: block_data.block.header.number,
            items: Vec::with_capacity(num_txs),
        };

        match block_data.block.transactions {
            BlockTransactions::Full(transactions) => {
                for tx in transactions {
                    let tx: Transaction = tx.into();
                    let tx_hash = tx.hash();

                    // Get the logs for this transaction, if any.
                    let raw_logs_for_tx =
                        block_data.logs.get(&tx_hash).cloned().unwrap_or_default();

                    let mut decoded_logs: Vec<DecodedLog> = Vec::new();
                    for log in &raw_logs_for_tx {
                        if monitor_snapshot.interest_registry.is_log_interesting(log) {
                            match self.abi_service.decode_log(log) {
                                Ok(decoded) => decoded_logs.push(decoded),
                                Err(e) =>
                                    if !matches!(e, AbiError::AbiNotFound(_)) {
                                        tracing::warn!(
                                            log_address = %log.address(),
                                            error = %e,
                                            "Could not decode log for a monitored address. Check if the ABI is correct."
                                        );
                                    },
                            }
                        }
                    }

                    // Get the receipt for this transaction, if available
                    let receipt = block_data.receipts.get(&tx_hash).cloned();

                    let correlated_item = CorrelatedBlockItem::new(tx, decoded_logs, receipt);

                    decoded_block.items.push(correlated_item);
                }

                Ok(decoded_block)
            }
            BlockTransactions::Hashes(_) | BlockTransactions::Uncle => {
                tracing::warn!("Full transactions are required for processing.");
                // For now, we'll just skip processing this block.
                Ok(decoded_block)
            }
        }
    }

    /// Processes multiple blocks in a batch for better performance.
    /// This method processes blocks sequentially with controlled concurrency
    /// to avoid overwhelming the system while maximizing throughput.
    pub async fn process_blocks_batch(
        &self,
        blocks: Vec<BlockData>,
    ) -> Result<Vec<DecodedBlockData>, BlockProcessorError> {
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

        let processing_futures =
            blocks.into_iter().map(|block_data| self.process_block(block_data));

        let results = join_all(processing_futures).await;

        let decoded_blocks =
            results.into_iter().collect::<result::Result<Vec<_>, BlockProcessorError>>();

        tracing::info!(total_decoded_blocks = count, "Batch processing completed.");

        decoded_blocks
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use alloy::primitives::{Address, B256, Bytes, U256, address, b256};
    use tempfile::tempdir;

    use super::*;
    use crate::{
        config::RhaiConfig,
        engine::rhai::RhaiCompiler,
        models::monitor::Monitor,
        test_helpers::{
            BlockBuilder, LogBuilder, MonitorBuilder, TransactionBuilder, create_test_abi_service,
            simple_abi_json,
        },
    };

    fn setup_abi_service_with_abi(abi_name: &str, abi_content: &str) -> (Arc<AbiService>, Address) {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[(abi_name, abi_content)]);
        let address = address!("0000000000000000000000000000000000000001");
        abi_service.link_abi(address, abi_name).unwrap();
        (abi_service, address)
    }

    fn setup_monitor_manager_with_monitors(
        monitors: Vec<Monitor>,
        abi_service: Arc<AbiService>,
    ) -> Arc<MonitorManager> {
        Arc::new(MonitorManager::new(
            monitors,
            Arc::new(RhaiCompiler::new(RhaiConfig::default())),
            abi_service,
        ))
    }

    #[tokio::test]
    async fn test_process_block_global_abi() {
        let block_number = 123;
        let contract_address = address!("0000000000000000000000000000000000000001");
        let tx_hash = b256!("1111111111111111111111111111111111111111111111111111111111111111");

        // 1. Setup ABI Service with a global ABI
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("simple", simple_abi_json())]);
        abi_service.add_global_abi("simple").unwrap();

        // Setup global monitor
        let monitors = vec![
            MonitorBuilder::new()
                .name("Global ABI Monitor")
                .address("all")
                .abi("simple")
                .filter_script("log.name == \"Transfer\"")
                .build(),
        ];

        // Setup MonitorManager
        let monitor_manager = setup_monitor_manager_with_monitors(monitors, abi_service.clone());

        // 2. Setup BlockData
        let tx = TransactionBuilder::new()
            .hash(tx_hash)
            .to(Some(contract_address))
            .block_number(block_number)
            .build();

        let log = LogBuilder::new()
            .address(contract_address)
            .topic(b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"))
            .topic(Address::default().into_word())
            .topic(Address::default().into_word())
            .data(Bytes::from(U256::from(1000).to_be_bytes::<32>()))
            .transaction_hash(tx_hash)
            .block_number(block_number)
            .build();

        let block = BlockBuilder::new().number(block_number).transaction(tx).build();

        let mut logs_by_tx = HashMap::new();
        logs_by_tx.insert(tx_hash, vec![log.into()]);

        let block_data = BlockData::new(block, HashMap::new(), logs_by_tx);

        // 3. Setup BlockProcessor
        let block_processor = BlockProcessor::new(abi_service, monitor_manager);

        // 4. Process block
        let decoded_block = block_processor.process_block(block_data).await.unwrap();

        // 5. Assertions
        assert_eq!(decoded_block.items[0].decoded_logs.len(), 1);
        assert_eq!(decoded_block.items[0].decoded_logs[0].name, "Transfer");
    }

    #[tokio::test]
    async fn test_process_block_happy_path() {
        let block_number = 123;
        let tx_hash = b256!("1111111111111111111111111111111111111111111111111111111111111111");

        // 1. Setup ABI Service
        let (abi_service, contract_address) =
            setup_abi_service_with_abi("simple", simple_abi_json());

        // Setup MonitorManager
        let monitors = vec![
            MonitorBuilder::new()
                .name("Transfer Monitor")
                .address(&contract_address.to_string())
                .abi("simple")
                .filter_script("log.name == \"Transfer\"")
                .build(),
        ];
        let monitor_manager = setup_monitor_manager_with_monitors(monitors, abi_service.clone());

        // 2. Setup BlockData
        let tx = TransactionBuilder::new()
            .hash(tx_hash)
            .to(Some(contract_address))
            .block_number(block_number)
            .build();

        let log = LogBuilder::new()
            .address(contract_address)
            .topic(b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"))
            .topic(Address::default().into_word())
            .topic(Address::default().into_word())
            .data(Bytes::from(U256::from(1000).to_be_bytes::<32>()))
            .transaction_hash(tx_hash)
            .block_number(block_number)
            .build();

        let block = BlockBuilder::new().number(block_number).transaction(tx).build();

        let mut logs_by_tx = HashMap::new();
        logs_by_tx.insert(tx_hash, vec![log.into()]);

        let block_data = BlockData::new(block, HashMap::new(), logs_by_tx);

        // 3. Setup BlockProcessor
        let block_processor = BlockProcessor::new(abi_service, monitor_manager);

        // 4. Process block
        let decoded_block = block_processor.process_block(block_data).await.unwrap();

        // 5. Assertions
        assert_eq!(decoded_block.items.len(), 1);
        let correlated_item = &decoded_block.items[0];
        assert_eq!(correlated_item.transaction.hash(), tx_hash);
        assert_eq!(correlated_item.transaction.block_number(), Some(block_number));
        assert_eq!(correlated_item.decoded_logs.len(), 1);
        let decoded_log = &correlated_item.decoded_logs[0];
        assert_eq!(decoded_log.name, "Transfer");
        assert_eq!(decoded_log.params.len(), 3);
        // TODO: consider more assertions
    }

    #[tokio::test]
    async fn test_process_block_no_full_transactions() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]);
        let monitor_manager = setup_monitor_manager_with_monitors(vec![], abi_service.clone());
        let block_processor = BlockProcessor::new(abi_service, monitor_manager);

        // Create a block with no full transactions
        let block = BlockBuilder::new().build();
        let block_data = BlockData::new(block, HashMap::new(), HashMap::new());
        let decoded_block = block_processor.process_block(block_data).await.unwrap();
        assert!(decoded_block.items.is_empty());
    }

    #[tokio::test]
    async fn test_process_block_log_decoding_error() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Empty repo
        let monitor_manager = setup_monitor_manager_with_monitors(vec![], abi_service.clone());
        let block_processor = BlockProcessor::new(abi_service, monitor_manager);
        let tx_hash = B256::default();
        let tx = TransactionBuilder::new().hash(tx_hash).build();
        let log = LogBuilder::new().transaction_hash(tx_hash).build();
        let block = BlockBuilder::new().transaction(tx).build();
        let mut logs_by_tx = HashMap::new();

        logs_by_tx.insert(tx_hash, vec![log.into()]);

        let block_data = BlockData::new(block, HashMap::new(), logs_by_tx);
        let decoded_block = block_processor.process_block(block_data).await.unwrap();

        for item in decoded_block.items {
            assert!(item.decoded_logs.is_empty(), "Expected no decoded logs due to decoding error");
        }
    }
    #[tokio::test]
    async fn test_process_blocks_batch_multiple_blocks() {
        let (abi_service, contract_address) =
            setup_abi_service_with_abi("simple", simple_abi_json());
        let monitor_manager = setup_monitor_manager_with_monitors(vec![], abi_service.clone());
        let block_processor = BlockProcessor::new(abi_service, monitor_manager);

        // Create multiple blocks for batch processing
        let mut blocks = Vec::new();

        for block_num in 100..103 {
            let tx_hash = B256::from([block_num as u8; 32]);
            let tx = TransactionBuilder::new()
                .hash(tx_hash)
                .to(Some(contract_address))
                .block_number(block_num)
                .build();

            let log = LogBuilder::new()
                .address(contract_address)
                .topic(b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"))
                .topic(Address::default().into_word())
                .topic(Address::default().into_word())
                .data(Bytes::from(U256::from(1000).to_be_bytes::<32>()))
                .transaction_hash(tx_hash)
                .block_number(block_num)
                .build();

            let block = BlockBuilder::new().number(block_num).transaction(tx).build();

            let mut logs_by_tx = HashMap::new();
            logs_by_tx.insert(tx_hash, vec![log.into()]);

            let block_data = BlockData::new(block, HashMap::new(), logs_by_tx);
            blocks.push(block_data);
        }

        // Process blocks in batch
        let decoded_blocks = block_processor.process_blocks_batch(blocks).await.unwrap();

        // Should have one match per block (3 total)
        assert_eq!(decoded_blocks.len(), 3);

        for decoded_block in &decoded_blocks {
            assert_eq!(decoded_block.items.len(), 1);
            // TODO: consider more assertions
        }
    }
    #[tokio::test]
    async fn test_process_blocks_batch_empty() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]);
        let monitor_manager = setup_monitor_manager_with_monitors(vec![], abi_service.clone());
        let block_processor = BlockProcessor::new(abi_service, monitor_manager);

        let matches = block_processor.process_blocks_batch(Vec::new()).await.unwrap();
        assert!(matches.is_empty());
    }

    #[tokio::test]
    async fn test_process_blocks_batch_no_full_transactions() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]);
        let monitor_manager = setup_monitor_manager_with_monitors(vec![], abi_service.clone());
        let block_processor = BlockProcessor::new(abi_service, monitor_manager);

        // Create a block with no full transactions
        let block = BlockBuilder::new().build();
        let block_data = BlockData::new(block, HashMap::new(), HashMap::new());

        let decoded_blocks = block_processor.process_blocks_batch(vec![block_data]).await.unwrap();

        for decoded_block in decoded_blocks {
            assert!(decoded_block.items.is_empty(), "Expected no items in decoded block");
        }
    }

    #[tokio::test]
    async fn test_process_blocks_batch_log_decoding_error() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Empty repo
        let monitor_manager = setup_monitor_manager_with_monitors(vec![], abi_service.clone());
        let block_processor = BlockProcessor::new(abi_service, monitor_manager);

        let tx_hash = B256::default();
        let tx = TransactionBuilder::new().hash(tx_hash).build();
        let log = LogBuilder::new().transaction_hash(tx_hash).build();
        let block = BlockBuilder::new().transaction(tx).build();

        let mut logs_by_tx = HashMap::new();
        logs_by_tx.insert(tx_hash, vec![log.into()]);
        let block_data = BlockData::new(block, HashMap::new(), logs_by_tx);

        // Should run without error, but no matches will be found as decoding fails
        // silently.
        let decoded_blocks = block_processor.process_blocks_batch(vec![block_data]).await.unwrap();

        for decoded_block in decoded_blocks {
            for item in decoded_block.items {
                assert!(
                    item.decoded_logs.is_empty(),
                    "Expected no decoded logs due to decoding error"
                );
            }
        }
    }
}
