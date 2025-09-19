//! The BlockIngestor module is responsible for continuously fetching block data
//! from a DataSource and ingesting it into the processing pipeline.

use std::{collections::HashMap, sync::Arc};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    config::AppConfig,
    engine::filtering::FilteringEngine,
    models::BlockData,
    persistence::traits::StateRepository,
    providers::traits::{DataSource, DataSourceError},
};

/// The BlockIngestor service.
///
/// This service runs a continuous loop to fetch new blocks from a `DataSource`,
/// respecting confirmation delays and the last processed state. It sends the
/// raw `BlockData` into a channel for the `BlockProcessor` to consume.
pub struct BlockIngestor<
    S: StateRepository + ?Sized,
    D: DataSource + ?Sized,
    F: FilteringEngine + ?Sized,
> {
    /// Shared application configuration.
    config: Arc<AppConfig>,
    /// The persistent state repository for managing application state.
    state: Arc<S>,
    /// The data source for fetching new blockchain data.
    data_source: Arc<D>,
    /// The filtering engine, used to check if receipt data is needed.
    filtering: Arc<F>,
    /// The sender for the raw block data channel.
    raw_blocks_tx: mpsc::Sender<BlockData>,
    /// A token used to signal a graceful shutdown.
    cancellation_token: CancellationToken,
}

impl<S: StateRepository + ?Sized, D: DataSource + ?Sized, F: FilteringEngine + ?Sized>
    BlockIngestor<S, D, F>
{
    /// Creates a new BlockIngestor instance.
    pub fn new(
        config: Arc<AppConfig>,
        state: Arc<S>,
        data_source: Arc<D>,
        filtering: Arc<F>,
        raw_blocks_tx: mpsc::Sender<BlockData>,
        cancellation_token: CancellationToken,
    ) -> Self {
        Self {
            config,
            state,
            data_source,
            filtering,
            raw_blocks_tx,
            cancellation_token,
        }
    }

    /// Starts the long-running service loop.
    pub async fn run(self) {
        loop {
            let polling_delay = tokio::time::sleep(self.config.polling_interval_ms);

            tokio::select! {
                biased;

                _ = self.cancellation_token.cancelled() => {
                    tracing::info!("BlockIngestor cancellation signal received, shutting down...");
                    break;
                }

                _ = polling_delay => {
                    if let Err(e) = self.ingest_blocks().await {
                        tracing::error!(error = %e, "Error during block ingestion cycle. Retrying after delay...");
                    }
                }
            }
        }
        tracing::info!("BlockIngestor has shut down.");
    }

    /// Performs one cycle of fetching and sending block data.
    async fn ingest_blocks(&self) -> Result<(), DataSourceError> {
        let network_id = &self.config.network_id;
        let last_processed_block = self.state.get_last_processed_block(network_id).await?;
        let current_block = self.data_source.get_current_block_number().await?;

        if current_block < self.config.confirmation_blocks {
            tracing::debug!("Chain is shorter than the confirmation buffer. Waiting for more blocks.");
            return Ok(());
        }

        let from_block =
            last_processed_block.map_or_else(|| current_block.saturating_sub(100), |block| block + 1);
        let safe_to_block = current_block.saturating_sub(self.config.confirmation_blocks);

        if from_block > safe_to_block {
            tracing::debug!("Caught up to confirmation buffer. Waiting for more blocks.");
            return Ok(());
        }

        let to_block = std::cmp::min(from_block + self.config.block_chunk_size, safe_to_block);
        tracing::info!(from_block = from_block, to_block = to_block, "Processing block range.");

        for block_num in from_block..=to_block {
            if self.cancellation_token.is_cancelled() {
                tracing::info!("Cancellation requested, stopping block ingestion.");
                break;
            }

            let (block, logs) = self.data_source.fetch_block_core_data(block_num).await?;
            let needs_receipts = self.filtering.requires_receipt_data();
            let receipts = if needs_receipts {
                let tx_hashes: Vec<_> = block.transactions.hashes().collect();
                if tx_hashes.is_empty() {
                    HashMap::new()
                } else {
                    self.data_source.fetch_receipts(&tx_hashes).await?
                }
            } else {
                HashMap::new()
            };

            let block_data = BlockData::from_raw_data(block, receipts, logs);

            if self.raw_blocks_tx.send(block_data).await.is_err() {
                tracing::warn!("Raw blocks channel closed, stopping further ingestion.");
                return Err(DataSourceError::ChannelClosed);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        engine::filtering::MockFilteringEngine,
        persistence::traits::MockStateRepository,
        providers::traits::MockDataSource,
        test_helpers::{BlockBuilder, ReceiptBuilder, TransactionBuilder},
    };
    use alloy::primitives::B256;
    use mockall::predicate::eq;

    const TEST_NETWORK_ID: &str = "testnet";

    struct TestHarness {
        config: Arc<AppConfig>,
        mock_state_repo: MockStateRepository,
        mock_data_source: MockDataSource,
        mock_filtering_engine: MockFilteringEngine,
    }

    impl TestHarness {
        fn new() -> Self {
            let config = Arc::new(
                AppConfig::builder().network_id(TEST_NETWORK_ID).confirmation_blocks(1).build(),
            );
            Self {
                config,
                mock_state_repo: MockStateRepository::new(),
                mock_data_source: MockDataSource::new(),
                mock_filtering_engine: MockFilteringEngine::new(),
            }
        }

        fn build(
            self,
            tx: mpsc::Sender<BlockData>,
            token: CancellationToken,
        ) -> BlockIngestor<MockStateRepository, MockDataSource, MockFilteringEngine> {
            BlockIngestor::new(
                self.config,
                Arc::new(self.mock_state_repo),
                Arc::new(self.mock_data_source),
                Arc::new(self.mock_filtering_engine),
                tx,
                token,
            )
        }
    }

    #[tokio::test]
    async fn test_ingest_blocks_succeeds_without_fetching_receipts_when_not_required() {
        let mut harness = TestHarness::new();
        harness.mock_filtering_engine.expect_requires_receipt_data().returning(|| false);
        harness.mock_state_repo.expect_get_last_processed_block().returning(|_| Ok(Some(121)));
        harness.mock_data_source.expect_get_current_block_number().returning(|| Ok(123));
        harness
            .mock_data_source
            .expect_fetch_block_core_data()
            .with(eq(122))
            .returning(|block_num| Ok((BlockBuilder::new().number(block_num).build(), vec![])));
        harness.mock_data_source.expect_fetch_receipts().times(0);

        let (tx, _rx) = mpsc::channel(10);
        let ingestor = harness.build(tx, CancellationToken::new());

        let result = ingestor.ingest_blocks().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_ingest_blocks_skips_receipt_fetch_for_empty_block() {
        let mut harness = TestHarness::new();
        harness.mock_filtering_engine.expect_requires_receipt_data().returning(|| true);
        harness.mock_state_repo.expect_get_last_processed_block().returning(|_| Ok(Some(121)));
        harness.mock_data_source.expect_get_current_block_number().returning(|| Ok(123));
        harness
            .mock_data_source
            .expect_fetch_block_core_data()
            .with(eq(122))
            .returning(|block_num| Ok((BlockBuilder::new().number(block_num).build(), vec![])));
        harness.mock_data_source.expect_fetch_receipts().times(0);

        let (tx, _rx) = mpsc::channel(10);
        let ingestor = harness.build(tx, CancellationToken::new());

        let result = ingestor.ingest_blocks().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_ingest_blocks_fetches_receipts_successfully_when_required() {
        let mut harness = TestHarness::new();
        let tx_hash = B256::from([1u8; 32]);
        let block = BlockBuilder::new().transaction(TransactionBuilder::new().hash(tx_hash).build());
        let receipt = ReceiptBuilder::new().transaction_hash(tx_hash).build();
        let mut expected_receipts = HashMap::new();
        expected_receipts.insert(tx_hash, receipt);

        harness.mock_filtering_engine.expect_requires_receipt_data().returning(|| true);
        harness.mock_state_repo.expect_get_last_processed_block().returning(|_| Ok(Some(121)));
        harness.mock_data_source.expect_get_current_block_number().returning(|| Ok(123));
        harness
            .mock_data_source
            .expect_fetch_block_core_data()
            .with(eq(122))
            .returning(move |block_num| Ok((block.clone().number(block_num).build(), vec![])));
        harness
            .mock_data_source
            .expect_fetch_receipts()
            .with(eq(vec![tx_hash]))
            .returning(move |_| Ok(expected_receipts.clone()));

        let (tx, _rx) = mpsc::channel(10);
        let ingestor = harness.build(tx, CancellationToken::new());

        let result = ingestor.ingest_blocks().await;
        assert!(result.is_ok());
    }
}