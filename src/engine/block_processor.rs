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
    persistence::traits::StateRepository,
};

/// The BlockProcessor service.
///
/// This service receives raw `BlockData` from a channel, buffers it into
/// batches of a configurable size, processes the batch into
/// `CorrelatedBlockData`, and then sends the result to the filtering engine's
/// channel. It is also responsible for updating the `last_processed_block`
/// state in the database.
pub struct BlockProcessor<S: StateRepository + ?Sized> {
    /// Shared application configuration.
    config: Arc<AppConfig>,
    /// The persistent state repository for managing application state.
    state: Arc<S>,
    /// The receiver for the raw block data channel.
    raw_blocks_rx: mpsc::Receiver<BlockData>,
    /// The sender for the correlated block data channel.
    correlated_blocks_tx: mpsc::Sender<CorrelatedBlockData>,
    /// A token used to signal a graceful shutdown.
    cancellation_token: CancellationToken,
}

impl<S: StateRepository + ?Sized> BlockProcessor<S> {
    /// Creates a new BlockProcessor instance.
    pub fn new(
        config: Arc<AppConfig>,
        state: Arc<S>,
        raw_blocks_rx: mpsc::Receiver<BlockData>,
        correlated_blocks_tx: mpsc::Sender<CorrelatedBlockData>,
        cancellation_token: CancellationToken,
    ) -> Self {
        Self { config, state, raw_blocks_rx, correlated_blocks_tx, cancellation_token }
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
        match process_blocks_batch(blocks).await {
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
            .await?;

    tracing::info!(total_correlated_blocks = count, "Batch correlation completed.");

    Ok(correlated_blocks)
}

/// Correlates a single block's data, grouping transactions with their logs and
/// receipts.
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use mockall::predicate::eq;

    use super::*;
    use crate::{
        persistence::traits::MockStateRepository,
        test_helpers::{BlockBuilder, TransactionBuilder},
    };

    const TEST_NETWORK_ID: &str = "testnet";

    struct TestHarness {
        config: Arc<AppConfig>,
        mock_state_repo: MockStateRepository,
    }

    impl TestHarness {
        fn new() -> Self {
            let config = Arc::new(AppConfig::builder().network_id(TEST_NETWORK_ID).build());
            Self { config, mock_state_repo: MockStateRepository::new() }
        }

        fn build(
            self,
            rx: mpsc::Receiver<BlockData>,
            tx: mpsc::Sender<CorrelatedBlockData>,
            token: CancellationToken,
        ) -> BlockProcessor<MockStateRepository> {
            BlockProcessor::new(self.config, Arc::new(self.mock_state_repo), rx, tx, token)
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
        let processor = harness.build(raw_rx, correlated_tx, CancellationToken::new());

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
}
