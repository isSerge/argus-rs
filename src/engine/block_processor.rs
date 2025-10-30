//! This module provides the `BlockProcessor` service, which is responsible for
//! processing raw blockchain data into a correlated format, ready for the
//! filtering engine.

use std::sync::Arc;

use alloy::rpc::types::BlockTransactions;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    config::AppConfig,
    models::{BlockData, CorrelatedBlockData, CorrelatedBlockItem, transaction::Transaction},
    monitor::{MonitorAssetState, MonitorManager},
    persistence::traits::AppRepository,
};

/// The BlockProcessor service.
///
/// This service receives raw `BlockData` from a channel, buffers it into
/// batches of a configurable size, processes the batch into
/// `CorrelatedBlockData`, and then sends the result to the filtering engine's
/// channel. It is also responsible for updating the `last_processed_block`
/// state in the database.
pub struct BlockProcessor<S: AppRepository + ?Sized> {
    /// Shared application configuration.
    config: Arc<AppConfig>,
    /// The persistent state repository for managing application state.
    state: Arc<S>,
    /// The monitor manager.
    monitor_manager: Arc<MonitorManager>,
    /// The receiver for the raw block data channel.
    raw_blocks_rx: mpsc::Receiver<BlockData>,
    /// The sender for the correlated block data channel.
    correlated_blocks_tx: mpsc::Sender<CorrelatedBlockData>,
    /// A token used to signal a graceful shutdown.
    cancellation_token: CancellationToken,
}

impl<S: AppRepository + ?Sized + Send + Sync> BlockProcessor<S> {
    /// Creates a new BlockProcessor instance.
    pub fn new(
        config: Arc<AppConfig>,
        state: Arc<S>,
        monitor_manager: Arc<MonitorManager>,
        raw_blocks_rx: mpsc::Receiver<BlockData>,
        correlated_blocks_tx: mpsc::Sender<CorrelatedBlockData>,
        cancellation_token: CancellationToken,
    ) -> Self {
        Self {
            config,
            state,
            monitor_manager,
            raw_blocks_rx,
            correlated_blocks_tx,
            cancellation_token,
        }
    }

    /// Starts the long-running service loop.
    pub async fn run(mut self) {
        let mut block_buffer = Vec::new();
        let batch_size = self.config.block_chunk_size as usize;

        loop {
            tokio::select! {
                biased;

                _ = self.cancellation_token.cancelled() => {
                    tracing::info!("BlockProcessor cancellation signal received, shutting down...");
                    break;
                }

                Some(block_data) = self.raw_blocks_rx.recv() => {
                    block_buffer.push(block_data);
                    if block_buffer.len() >= batch_size {
                        let batch_to_process = std::mem::take(&mut block_buffer);
                        self.process_and_dispatch(batch_to_process).await;
                    }
                }
            }
        }

        // Process any remaining blocks in the buffer before shutting down.
        if !block_buffer.is_empty() {
            self.process_and_dispatch(block_buffer).await;
        }
        tracing::info!("BlockProcessor has shut down.");
    }

    /// Processes a batch of blocks and dispatches them to the filtering engine.
    async fn process_and_dispatch(&self, blocks: Vec<BlockData>) {
        match process_blocks_batch(blocks, self.monitor_manager.clone()).await {
            Ok(correlated_blocks) => {
                let mut last_processed = None;
                for correlated_block in correlated_blocks {
                    let block_num = correlated_block.block_number;
                    if self.correlated_blocks_tx.send(correlated_block).await.is_err() {
                        tracing::warn!(
                            "Correlated blocks channel closed, stopping further processing."
                        );
                        return;
                    }
                    last_processed = Some(block_num);
                }

                if let Some(valid_last_processed) = last_processed {
                    if let Err(e) = self
                        .state
                        .set_last_processed_block(&self.config.network_id, valid_last_processed)
                        .await
                    {
                        tracing::error!(error = %e, "Failed to set last processed block.");
                    } else {
                        tracing::info!(
                            last_processed_block = valid_last_processed,
                            "Last processed block updated successfully."
                        );
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "Error during batch processing, will retry.");
            }
        }
    }
}

/// Processes a batch of `BlockData` into `CorrelatedBlockData`.
///
/// This function remains separate from the service to allow for easier testing
/// and potential reuse.
pub async fn process_blocks_batch(
    blocks: Vec<BlockData>,
    monitor_manager: Arc<MonitorManager>,
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

    let correlated_blocks: Vec<CorrelatedBlockData> = tokio::task::spawn_blocking(move || {
        let monitor_snapshot = monitor_manager.load();
        blocks.into_iter().map(|block| process_block(block, &monitor_snapshot)).collect()
    })
    .await?;

    tracing::info!(total_correlated_blocks = count, "Batch correlation completed.");

    Ok(correlated_blocks)
}

/// Correlates a single block's data, grouping transactions with their logs and
/// receipts.
fn process_block(
    block_data: BlockData,
    monitor_snapshot: &MonitorAssetState,
) -> CorrelatedBlockData {
    let mut correlated_items = Vec::new();
    let interest_registry = &monitor_snapshot.interest_registry;

    if let BlockTransactions::Full(transactions) = block_data.block.transactions {
        correlated_items.reserve(transactions.len());

        // Choose the processing strategy based on whether any transaction-only monitors
        // exist.
        if monitor_snapshot.has_transaction_only_monitors {
            // Fast Path: If there's a global transaction monitor, we can't skip any
            // transaction. This path avoids the overhead of checking each
            // transaction for potential matches.
            for tx in transactions {
                let tx: Transaction = tx.into();
                let tx_hash = tx.hash();
                let logs = block_data.logs.get(&tx_hash).cloned().unwrap_or_default();
                let receipt = block_data.receipts.get(&tx_hash).cloned();
                let correlated_item = CorrelatedBlockItem::new(tx, logs, receipt);
                correlated_items.push(correlated_item);
            }
        } else {
            // Optimized Path: No global transaction monitors, so we can inspect each
            // transaction and skip those that are not potentially matchable.
            for tx in transactions {
                let tx: Transaction = tx.into();
                let tx_hash = tx.hash();
                let logs = block_data.logs.get(&tx_hash).cloned().unwrap_or_default();
                let to_addr = tx.to();

                // A transaction is "potentially matchable" if it has logs or is directed at an
                // address we're monitoring for calldata.
                let is_potentially_matchable = !logs.is_empty()
                    || (to_addr.is_some()
                        && interest_registry.calldata_addresses.contains(&to_addr.unwrap()));

                if is_potentially_matchable {
                    let receipt = block_data.receipts.get(&tx_hash).cloned();
                    let correlated_item = CorrelatedBlockItem::new(tx, logs, receipt);
                    correlated_items.push(correlated_item);
                }
            }
        }
    } else {
        tracing::warn!(
            block_number = block_data.block.header.number,
            "Block contains only transaction hashes, not full transaction data."
        );
    }

    CorrelatedBlockData { block_number: block_data.block.header.number, items: correlated_items }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use alloy::primitives::{B256, address};
    use mockall::predicate::eq;

    use super::*;
    use crate::{
        monitor::InterestRegistry,
        persistence::traits::MockAppRepository,
        test_helpers::{
            BlockBuilder, LogBuilder, MonitorBuilder, TransactionBuilder,
            create_test_monitor_manager,
        },
    };

    const TEST_NETWORK_ID: &str = "testnet";

    struct TestHarness {
        config: Arc<AppConfig>,
        mock_state_repo: MockAppRepository,
    }

    impl TestHarness {
        fn new() -> Self {
            let config = Arc::new(AppConfig::builder().network_id(TEST_NETWORK_ID).build());
            Self { config, mock_state_repo: MockAppRepository::new() }
        }

        async fn build(
            self,
            rx: mpsc::Receiver<BlockData>,
            tx: mpsc::Sender<CorrelatedBlockData>,
            token: CancellationToken,
        ) -> BlockProcessor<MockAppRepository> {
            let monitor =
                MonitorBuilder::new().network(TEST_NETWORK_ID).filter_script("true").build();
            let monitors = vec![monitor];
            let monitor_manager = create_test_monitor_manager(monitors).await;
            BlockProcessor::new(
                self.config,
                Arc::new(self.mock_state_repo),
                monitor_manager,
                rx,
                tx,
                token,
            )
        }
    }

    #[tokio::test]
    async fn test_process_and_dispatch_sends_correlated_data_and_updates_state() {
        let mut harness = TestHarness::new();
        harness
            .mock_state_repo
            .expect_set_last_processed_block()
            .with(eq(TEST_NETWORK_ID), eq(100))
            .times(1)
            .returning(|_, _| Ok(()));

        let (_, raw_rx) = mpsc::channel(10);
        let (correlated_tx, mut correlated_rx) = mpsc::channel(10);
        let processor = harness.build(raw_rx, correlated_tx, CancellationToken::new()).await;

        let block_number = 100;
        let block = BlockBuilder::new()
            .number(block_number)
            .transaction(TransactionBuilder::new().build())
            .build();
        let block_data = BlockData::from_raw_data(block, HashMap::new(), vec![]);

        processor.process_and_dispatch(vec![block_data]).await;

        let correlated_block = correlated_rx.recv().await.unwrap();
        assert_eq!(correlated_block.block_number, block_number);
    }

    // Tests for the process_block function
    fn empty_monitor_snapshot() -> MonitorAssetState {
        MonitorAssetState {
            monitors_by_id: HashMap::new(),
            log_aware_monitors: vec![],
            tx_aware_monitors: vec![],
            requires_receipts: false,
            interest_registry: InterestRegistry::default(),
            has_transaction_only_monitors: false,
        }
    }

    #[test]
    fn test_fast_path_processes_all_transactions() {
        // Arrange
        let mut monitor_snapshot = empty_monitor_snapshot();
        monitor_snapshot.has_transaction_only_monitors = true;

        let tx1 = TransactionBuilder::new().build();
        let tx2 = TransactionBuilder::new().build();
        let block = BlockBuilder::new().number(100).transaction(tx1).transaction(tx2).build();
        let block_data = BlockData::from_raw_data(block, HashMap::new(), vec![]);

        // Act
        let result = process_block(block_data, &monitor_snapshot);

        // Assert
        assert_eq!(result.items.len(), 2, "All transactions should be processed on the fast path");
    }

    #[test]
    fn test_optimized_path_filters_irrelevant_transactions() {
        // Arrange
        let mut monitor_snapshot = empty_monitor_snapshot();
        let monitored_addr = address!("0000000000000000000000000000000000000001");
        monitor_snapshot.interest_registry.calldata_addresses =
            HashSet::from([monitored_addr]).into();

        // Create transactions with unique hashes
        let hash1 = B256::from([1u8; 32]);
        let hash2 = B256::from([2u8; 32]);
        let hash3 = B256::from([3u8; 32]);

        let tx_with_log = TransactionBuilder::new().hash(hash1).build();
        let tx_to_monitored_addr =
            TransactionBuilder::new().hash(hash2).to(Some(monitored_addr)).build();
        // Ensure the irrelevant transaction does not accidentally match the monitored
        // address.
        let irrelevant_addr = address!("0000000000000000000000000000000000000002");
        let irrelevant_tx = TransactionBuilder::new().hash(hash3).to(Some(irrelevant_addr)).build();

        let block = BlockBuilder::new()
            .number(100)
            .transaction(tx_with_log.clone())
            .transaction(tx_to_monitored_addr.clone())
            .transaction(irrelevant_tx)
            .build();

        // `from_raw_data` expects a Vec of raw Alloy logs, not a HashMap.
        let log = LogBuilder::new().transaction_hash(tx_with_log.hash()).build();
        let raw_logs = vec![log.into()];
        let block_data = BlockData::from_raw_data(block, HashMap::new(), raw_logs);

        // Act
        let result = process_block(block_data, &monitor_snapshot);

        // Assert
        assert_eq!(
            result.items.len(),
            2,
            "Should only process transactions with logs or to a monitored address"
        );
        assert_eq!(result.items[0].transaction, tx_with_log);
        assert_eq!(result.items[1].transaction, tx_to_monitored_addr);
    }

    #[test]
    fn test_optimized_path_processes_nothing_when_no_matches() {
        // Arrange
        let monitor_snapshot = empty_monitor_snapshot();

        let tx1 = TransactionBuilder::new().build();
        let tx2 = TransactionBuilder::new().build();
        let block = BlockBuilder::new().number(100).transaction(tx1).transaction(tx2).build();
        let block_data = BlockData::from_raw_data(block, HashMap::new(), vec![]);

        // Act
        let result = process_block(block_data, &monitor_snapshot);

        // Assert
        assert!(
            result.items.is_empty(),
            "No transactions should be processed when none are relevant"
        );
    }

    #[test]
    fn test_process_block_with_no_transactions() {
        // Arrange
        let monitor_snapshot = empty_monitor_snapshot();
        let block = BlockBuilder::new().number(100).build();
        let block_data = BlockData::from_raw_data(block, HashMap::new(), vec![]);

        // Act
        let result = process_block(block_data, &monitor_snapshot);

        // Assert
        assert!(result.items.is_empty());
    }
}
