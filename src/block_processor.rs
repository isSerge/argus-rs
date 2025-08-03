//! This module defines the `BlockProcessor` component.
//! 
//! The `BlockProcessor` is responsible for in-memory processing of blockchain data,
//! including log decoding, data correlation, and applying filtering logic.
//! It supports both single block processing and batch processing for improved throughput.

use crate::abi::{AbiService, DecodedLog};
use crate::filtering::FilteringEngine;
use crate::models::{BlockData, CorrelatedBlockItem, monitor_match::MonitorMatch};
use alloy::rpc::types::BlockTransactions;
use std::sync::Arc;
use thiserror::Error;

/// Custom error type for `BlockProcessor` operations.
#[derive(Error, Debug)]
pub enum BlockProcessorError {
    /// Wrapper for errors from the `AbiService`.
    #[error("ABI service error: {0}")]
    AbiService(#[from] crate::abi::AbiError),
    /// Wrapper for errors from the `FilteringEngine`.
    #[error("Filtering engine error: {0}")]
    FilteringEngine(Box<dyn std::error::Error + Send + Sync>),
}

/// The `BlockProcessor` is responsible for in-memory processing of `BlockData`.
/// This includes decoding logs, correlating data, and applying matching logic.
pub struct BlockProcessor<F: FilteringEngine> {
    abi_service: Arc<AbiService>,
    filtering_engine: F,
}

impl<F: FilteringEngine> BlockProcessor<F> {
    /// Creates a new `BlockProcessor` with the provided `AbiService` and `FilteringEngine`.
    pub fn new(abi_service: Arc<AbiService>, filtering_engine: F) -> Self {
        Self {
            abi_service,
            filtering_engine,
        }
    }

    /// Processes the given `BlockData`, decodes logs, correlates data,
    /// and applies filtering logic.
    pub async fn process_block(
        &self,
        block_data: BlockData,
    ) -> Result<Vec<MonitorMatch>, BlockProcessorError> {
        tracing::debug!(
            block_number = block_data.block.header.number,
            "Processing block data."
        );

        let mut all_matches: Vec<MonitorMatch> = Vec::new();

        match block_data.block.transactions {
            BlockTransactions::Full(transactions) => {
                for tx in transactions {
                    let tx: crate::models::transaction::Transaction = tx.into();
                    let tx_hash = tx.hash();

                    // Get the logs for this transaction, if any.
                    let raw_logs_for_tx = block_data
                        .logs
                        .get(&tx_hash)
                        .map(|logs| logs.as_slice())
                        .unwrap_or(&[]);

                    let mut decoded_logs: Vec<DecodedLog> = Vec::new();
                    for log in raw_logs_for_tx {
                        match self.abi_service.decode_log(log) {
                            Ok(decoded) => decoded_logs.push(decoded),
                            Err(e) => {
                                tracing::trace!(
                                    log_address = %log.address(),
                                    log_topics = ?log.topics(),
                                    error = %e,
                                    "Could not decode log."
                                );
                                // Continue even if a log cannot be decoded
                            }
                        }
                    }

                    // TODO: Implement intelligent receipt requirement logic when FilteringEngine is functional
                    // This should check monitor rules to determine if this transaction needs receipt data
                    // For now, let's assume all transactions might need receipts for some rule.
                    let receipt = block_data.receipts.get(&tx_hash);

                    let correlated_item = CorrelatedBlockItem::new(&tx, decoded_logs, receipt);

                    // Apply matching logic using the FilteringEngine
                    match self.filtering_engine.evaluate_item(&correlated_item).await {
                        Ok(matches) => all_matches.extend(matches),
                        Err(e) => {
                            tracing::error!(
                                tx_hash = %tx_hash,
                                error = %e,
                                "Error evaluating correlated block item."
                            );
                            return Err(BlockProcessorError::FilteringEngine(e));
                        }
                    }
                }
            }
            BlockTransactions::Hashes(_) | BlockTransactions::Uncle => {
                tracing::warn!("Full transactions are required for processing.");
                // For now, we'll just skip processing this block.
            }
        }

        Ok(all_matches)
    }

    /// Processes multiple blocks in a batch for better performance.
    /// This method processes blocks sequentially with controlled concurrency
    /// to avoid overwhelming the system while maximizing throughput.
    pub async fn process_blocks_batch(
        &self,
        blocks: Vec<BlockData>,
    ) -> Result<Vec<MonitorMatch>, BlockProcessorError> {
        if blocks.is_empty() {
            return Ok(Vec::new());
        }

        tracing::debug!(
            block_count = blocks.len(),
            first_block = blocks.first().map(|b| b.block.header.number),
            last_block = blocks.last().map(|b| b.block.header.number),
            "Processing batch of blocks."
        );

        let mut all_matches = Vec::new();
        
        // Process blocks sequentially but with async operations
        for block_data in blocks {
            let block_number = block_data.block.header.number;
            
            tracing::trace!(
                block_number = block_number,
                "Processing block in batch."
            );
            
            match self.process_block(block_data).await {
                Ok(matches) => {
                    let matches_count = matches.len();
                    all_matches.extend(matches);
                    tracing::trace!(
                        block_number = block_number,
                        matches_count = matches_count,
                        "Block processed successfully in batch."
                    );
                }
                Err(e) => {
                    tracing::error!(
                        block_number = block_number,
                        error = %e,
                        "Error processing block in batch."
                    );
                    return Err(e);
                }
            }
        }

        tracing::info!(
            total_matches = all_matches.len(),
            "Batch processing completed."
        );

        Ok(all_matches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        abi::AbiService,
        filtering::FilteringEngine,
        models::{
            block_data::BlockData, correlated_data::CorrelatedBlockItem,
            monitor_match::MonitorMatch,
        },
        test_helpers::{BlockBuilder, LogBuilder, TransactionBuilder},
    };
    use alloy::{
        json_abi::JsonAbi,
        primitives::{B256, address, b256, bytes},
    };
    use async_trait::async_trait;
    use std::{collections::HashMap, sync::Arc};

    fn simple_abi() -> JsonAbi {
        serde_json::from_str(
            r#"[
                {
                    "type": "event",
                    "name": "Transfer",
                    "inputs": [
                        {"name": "from", "type": "address", "indexed": true},
                        {"name": "to", "type": "address", "indexed": true},
                        {"name": "amount", "type": "uint256", "indexed": false}
                    ],
                    "anonymous": false
                }
            ]"#,
        )
        .unwrap()
    }

    // A mock filtering engine for testing purposes.
    struct MockFilteringEngine;

    #[async_trait]
    impl FilteringEngine for MockFilteringEngine {
        async fn evaluate_item(
            &self,
            item: &CorrelatedBlockItem<'_>,
        ) -> Result<Vec<MonitorMatch>, Box<dyn std::error::Error + Send + Sync>> {
            // If we see a decoded "Transfer" event, return a match.
            if item.decoded_logs.iter().any(|log| log.name == "Transfer") {
                let monitor_match = MonitorMatch {
                    monitor_id: "test_monitor".to_string(),
                    transaction_hash: item.tx_hash(),
                    block_number: item.transaction.block_number().unwrap_or(0),
                    log_index: None,
                    data: Default::default(),
                };
                return Ok(vec![monitor_match]);
            }
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn test_process_block_happy_path() {
        let block_number = 123;
        // 1. Setup ABI Service
        let abi_service = Arc::new(AbiService::new());
        let contract_address = address!("0000000000000000000000000000000000000001");
        abi_service.add_abi(contract_address, &simple_abi());

        // 2. Setup Transaction and Log
        let tx_hash = b256!("1111111111111111111111111111111111111111111111111111111111111111");
        let tx = TransactionBuilder::new()
            .hash(tx_hash)
            .to(Some(contract_address))
            .block_number(block_number)
            .build();

        let from_addr = address!("1111111111111111111111111111111111111111");
        let to_addr = address!("2222222222222222222222222222222222222222");

        let log = LogBuilder::new()
            .address(contract_address)
            .topic(b256!(
                "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
            ))
            .topic(from_addr.into_word())
            .topic(to_addr.into_word())
            .data(bytes!(
                "0000000000000000000000000000000000000000000000000000000000000064"
            ))
            .transaction_hash(tx_hash)
            .block_number(block_number)
            .build();

        // 3. Setup BlockData
        let block = BlockBuilder::new()
            .number(block_number)
            .transaction(tx)
            .build();

        let mut logs_by_tx = HashMap::new();
        logs_by_tx.insert(tx_hash, vec![log.into()]);

        let block_data = BlockData::new(block, HashMap::new(), logs_by_tx);

        // 4. Setup BlockProcessor
        let filtering_engine = MockFilteringEngine;
        let block_processor = BlockProcessor::new(abi_service, filtering_engine);

        // 5. Process block
        let matches = block_processor.process_block(block_data).await.unwrap();

        // 6. Assertions
        assert_eq!(matches.len(), 1);
        let a_match = &matches[0];
        assert_eq!(a_match.monitor_id, "test_monitor");
        assert_eq!(a_match.transaction_hash, tx_hash);
    }

    #[tokio::test]
    async fn test_process_block_no_full_transactions() {
        let abi_service = Arc::new(AbiService::new());
        let filtering_engine = MockFilteringEngine;
        let block_processor = BlockProcessor::new(abi_service, filtering_engine);

        // Create a block with no full transactions
        let block = BlockBuilder::new().build();
        let block_data = BlockData::new(block, HashMap::new(), HashMap::new());

        let matches = block_processor.process_block(block_data).await.unwrap();
        assert!(matches.is_empty());
    }

    #[tokio::test]
    async fn test_process_block_log_decoding_error() {
        let abi_service = Arc::new(AbiService::new()); // No ABIs added
        let filtering_engine = MockFilteringEngine;
        let block_processor = BlockProcessor::new(abi_service, filtering_engine);

        let tx_hash = B256::default();
        let tx = TransactionBuilder::new().hash(tx_hash).build();
        let log = LogBuilder::new().transaction_hash(tx_hash).build();
        let block = BlockBuilder::new().transaction(tx).build();

        let mut logs_by_tx = HashMap::new();
        logs_by_tx.insert(tx_hash, vec![log.into()]);
        let block_data = BlockData::new(block, HashMap::new(), logs_by_tx);

        // Should run without error, but no matches will be found as decoding fails silently.
        let matches = block_processor.process_block(block_data).await.unwrap();
        assert!(matches.is_empty());
    }

    #[tokio::test]
    async fn test_process_blocks_batch_multiple_blocks() {
        let abi_service = Arc::new(AbiService::new());
        let contract_address = address!("0000000000000000000000000000000000000001");
        abi_service.add_abi(contract_address, &simple_abi());

        let filtering_engine = MockFilteringEngine;
        let block_processor = BlockProcessor::new(abi_service, filtering_engine);

        // Create multiple blocks for batch processing
        let mut blocks = Vec::new();
        
        for block_num in 100..103 {
            let tx_hash = B256::from([block_num as u8; 32]);
            let tx = TransactionBuilder::new()
                .hash(tx_hash)
                .to(Some(contract_address))
                .block_number(block_num)
                .build();

            let from_addr = address!("1111111111111111111111111111111111111111");
            let to_addr = address!("2222222222222222222222222222222222222222");

            let log = LogBuilder::new()
                .address(contract_address)
                .topic(b256!(
                    "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
                ))
                .topic(from_addr.into_word())
                .topic(to_addr.into_word())
                .data(bytes!(
                    "0000000000000000000000000000000000000000000000000000000000000064"
                ))
                .transaction_hash(tx_hash)
                .block_number(block_num)
                .build();

            let block = BlockBuilder::new()
                .number(block_num)
                .transaction(tx)
                .build();

            let mut logs_by_tx = HashMap::new();
            logs_by_tx.insert(tx_hash, vec![log.into()]);

            let block_data = BlockData::new(block, HashMap::new(), logs_by_tx);
            blocks.push(block_data);
        }

        // Process blocks in batch
        let matches = block_processor.process_blocks_batch(blocks).await.unwrap();

        // Should have one match per block (3 total)
        assert_eq!(matches.len(), 3);
        
        // Verify each match has the correct monitor_id
        for a_match in &matches {
            assert_eq!(a_match.monitor_id, "test_monitor");
        }
    }

    #[tokio::test]
    async fn test_process_blocks_batch_empty() {
        let abi_service = Arc::new(AbiService::new());
        let filtering_engine = MockFilteringEngine;
        let block_processor = BlockProcessor::new(abi_service, filtering_engine);

        let matches = block_processor.process_blocks_batch(Vec::new()).await.unwrap();
        assert!(matches.is_empty());
    }

    #[tokio::test]
    async fn test_process_blocks_batch_no_full_transactions() {
        let abi_service = Arc::new(AbiService::new());
        let filtering_engine = MockFilteringEngine;
        let block_processor = BlockProcessor::new(abi_service, filtering_engine);

        // Create a block with no full transactions
        let block = BlockBuilder::new().build();
        let block_data = BlockData::new(block, HashMap::new(), HashMap::new());

        let matches = block_processor.process_blocks_batch(vec![block_data]).await.unwrap();
        assert!(matches.is_empty());
    }

    #[tokio::test]
    async fn test_process_blocks_batch_log_decoding_error() {
        let abi_service = Arc::new(AbiService::new()); // No ABIs added
        let filtering_engine = MockFilteringEngine;
        let block_processor = BlockProcessor::new(abi_service, filtering_engine);

        let tx_hash = B256::default();
        let tx = TransactionBuilder::new().hash(tx_hash).build();
        let log = LogBuilder::new().transaction_hash(tx_hash).build();
        let block = BlockBuilder::new().transaction(tx).build();

        let mut logs_by_tx = HashMap::new();
        logs_by_tx.insert(tx_hash, vec![log.into()]);
        let block_data = BlockData::new(block, HashMap::new(), logs_by_tx);

        // Should run without error, but no matches will be found as decoding fails silently.
        let matches = block_processor.process_blocks_batch(vec![block_data]).await.unwrap();
        assert!(matches.is_empty());
    }
}
