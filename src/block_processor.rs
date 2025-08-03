//! This module defines the `BlockProcessor` component.

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

                    // Identify transactions that *would* require full receipts (placeholder logic)
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
        test_helpers::TransactionBuilder,
    };
    use alloy::{
        json_abi::JsonAbi,
        primitives::{B256, LogData, address, b256, bytes},
        rpc::types::{Block, BlockTransactions, Header, Log as AlloyLog},
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
        // 1. Setup ABI Service
        let abi_service = Arc::new(AbiService::new());
        let contract_address = address!("0000000000000000000000000000000000000001");
        abi_service.add_abi(contract_address, &simple_abi());

        // 2. Setup Transaction and Log
        let tx_hash = b256!("1111111111111111111111111111111111111111111111111111111111111111");
        let tx = TransactionBuilder::new()
            .hash(tx_hash)
            .to(Some(contract_address))
            .block_number(123)
            .build();

        let from_addr = address!("1111111111111111111111111111111111111111");
        let to_addr = address!("2222222222222222222222222222222222222222");

        let log = AlloyLog {
            inner: alloy::primitives::Log {
                address: contract_address,
                data: LogData::new_unchecked(
                    vec![
                        b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"), // Transfer event signature
                        from_addr.into_word(),
                        to_addr.into_word(),
                    ],
                    bytes!("0000000000000000000000000000000000000000000000000000000000000064"), // amount = 100
                ),
            },
            transaction_hash: Some(tx_hash),
            block_number: Some(123),
            ..Default::default()
        };

        // 3. Setup BlockData
        let mut header: Header = Header::default();
        header.number = 123;

        let block = Block {
            header,
            transactions: BlockTransactions::Full(vec![tx.0]),
            ..Default::default()
        };
        let mut logs_by_tx = HashMap::new();
        logs_by_tx.insert(tx_hash, vec![log]);

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

        let block = Block {
            transactions: BlockTransactions::Hashes(vec![B256::default()]),
            ..Default::default()
        };
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
        let log = AlloyLog {
            transaction_hash: Some(tx_hash),
            ..Default::default()
        };

        let block = Block {
            transactions: BlockTransactions::Full(vec![tx.0]),
            ..Default::default()
        };
        let mut logs_by_tx = HashMap::new();
        logs_by_tx.insert(tx_hash, vec![log]);
        let block_data = BlockData::new(block, HashMap::new(), logs_by_tx);

        // Should run without error, but no matches will be found as decoding fails silently.
        let matches = block_processor.process_block(block_data).await.unwrap();
        assert!(matches.is_empty());
    }
}
