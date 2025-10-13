//! The BlockIngestor module is responsible for continuously fetching block data
//! from a DataSource and ingesting it into the processing pipeline.

use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    config::AppConfig,
    context::AppMetrics,
    engine::filtering::FilteringEngine,
    models::BlockData,
    persistence::traits::AppRepository,
    providers::traits::{DataSource, DataSourceError},
};

/// The BlockIngestor service.
///
/// This service runs a continuous loop to fetch new blocks from a `DataSource`,
/// respecting confirmation delays and the last processed state. It sends the
/// raw `BlockData` into a channel for the `BlockProcessor` to consume.
pub struct BlockIngestor<
    S: AppRepository + ?Sized,
    D: DataSource + ?Sized,
    F: FilteringEngine + ?Sized,
> {
    /// Shared application configuration.
    config: Arc<AppConfig>,
    /// The persistent state repository for managing application state.
    state: Arc<S>,
    /// The shared application metrics.
    app_metrics: AppMetrics,
    /// The data source for fetching new blockchain data.
    data_source: Arc<D>,
    /// The filtering engine, used to check if receipt data is needed.
    filtering: Arc<F>,
    /// The sender for the raw block data channel.
    raw_blocks_tx: mpsc::Sender<BlockData>,
    /// A token used to signal a graceful shutdown.
    cancellation_token: CancellationToken,
}

impl<S: AppRepository + ?Sized, D: DataSource + ?Sized, F: FilteringEngine + ?Sized>
    BlockIngestor<S, D, F>
{
    /// Creates a new BlockIngestor instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: Arc<AppConfig>,
        state: Arc<S>,
        app_metrics: AppMetrics,
        data_source: Arc<D>,
        filtering: Arc<F>,
        raw_blocks_tx: mpsc::Sender<BlockData>,
        cancellation_token: CancellationToken,
    ) -> Self {
        Self {
            config,
            state,
            app_metrics,
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
            tracing::debug!(
                "Chain is shorter than the confirmation buffer. Waiting for more blocks."
            );
            return Ok(());
        }

        let from_block = last_processed_block
            .map_or_else(|| current_block.saturating_sub(100), |block| block + 1);
        let safe_to_block = current_block.saturating_sub(self.config.confirmation_blocks);

        if from_block > safe_to_block {
            tracing::debug!("Caught up to confirmation buffer. Waiting for more blocks.");
            return Ok(());
        }

        let to_block = std::cmp::min(from_block + self.config.block_chunk_size, safe_to_block);
        tracing::info!(from_block = from_block, to_block = to_block, "Processing block range.");

        // Use the same batch processing approach as dry_run for consistency
        let block_data_batch = crate::providers::block_fetcher::fetch_blocks_concurrent(
            self.data_source.as_ref(),
            &self.filtering.requires_receipt_data(),
            from_block,
            to_block,
            self.config.concurrency as usize,
        )
        .await?;

        // Process each block in the successfully fetched batch
        for block_data in block_data_batch {
            if self.cancellation_token.is_cancelled() {
                tracing::info!("Cancellation requested, stopping block ingestion.");
                break;
            }

            let block_timestamp = block_data.block.header.timestamp;
            let block_num = block_data.block.header.number;

            if self.raw_blocks_tx.send(block_data).await.is_err() {
                tracing::warn!("Raw blocks channel closed, stopping further ingestion.");
                return Err(DataSourceError::ChannelClosed);
            }

            let mut metrics = self.app_metrics.metrics.write().await;
            metrics.latest_processed_block = block_num;
            metrics.latest_processed_block_timestamp_secs = block_timestamp;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use alloy::primitives::B256;
    use mockall::predicate::eq;

    use super::*;
    use crate::{
        engine::filtering::MockFilteringEngine,
        persistence::traits::MockAppRepository,
        providers::traits::MockDataSource,
        test_helpers::{BlockBuilder, ReceiptBuilder, TransactionBuilder},
    };

    const TEST_NETWORK_ID: &str = "testnet";

    struct TestHarness {
        config: Arc<AppConfig>,
        mock_state_repo: MockAppRepository,
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
                mock_state_repo: MockAppRepository::new(),
                mock_data_source: MockDataSource::new(),
                mock_filtering_engine: MockFilteringEngine::new(),
            }
        }

        fn build(
            self,
            tx: mpsc::Sender<BlockData>,
            token: CancellationToken,
        ) -> BlockIngestor<MockAppRepository, MockDataSource, MockFilteringEngine> {
            BlockIngestor::new(
                self.config,
                Arc::new(self.mock_state_repo),
                AppMetrics::default(),
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
        // The batch will process block 122 (from 122 to 122)
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
        // The batch will process block 122 (from 122 to 122) - empty block so no
        // receipts
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
        let block =
            BlockBuilder::new().transaction(TransactionBuilder::new().hash(tx_hash).build());
        let receipt = ReceiptBuilder::new().transaction_hash(tx_hash).build();
        let mut expected_receipts = HashMap::new();
        expected_receipts.insert(tx_hash, receipt);

        harness.mock_filtering_engine.expect_requires_receipt_data().returning(|| true);
        harness.mock_state_repo.expect_get_last_processed_block().returning(|_| Ok(Some(121)));
        harness.mock_data_source.expect_get_current_block_number().returning(|| Ok(123));
        // The batch will process block 122 (from 122 to 122) - block with transaction
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

    #[tokio::test]
    async fn test_ingestor_waits_when_caught_up_to_confirmation_head() {
        // This test ensures that if `from_block` > `safe_to_block`, we fetch nothing.
        let mut harness = TestHarness::new();
        // last processed is 121, next should be 122
        harness.mock_state_repo.expect_get_last_processed_block().returning(|_| Ok(Some(121)));
        // current is 122, so safe_to_block is 121
        harness.mock_data_source.expect_get_current_block_number().returning(|| Ok(122));
        // We should not attempt to fetch any block data
        harness.mock_data_source.expect_fetch_block_core_data().times(0);

        let (tx, _rx) = mpsc::channel(10);
        let ingestor = harness.build(tx, CancellationToken::new());

        let result = ingestor.ingest_blocks().await;
        assert!(result.is_ok(), "Should return Ok(()) even if no blocks are processed");
    }

    #[tokio::test]
    async fn test_ingestor_handles_data_source_error_gracefully() {
        let mut harness = TestHarness::new();
        harness.mock_state_repo.expect_get_last_processed_block().returning(|_| Ok(Some(100)));
        harness
            .mock_data_source
            .expect_get_current_block_number()
            .returning(|| Err(DataSourceError::BlockNotFound(123)));

        let (tx, _rx) = mpsc::channel(10);
        let ingestor = harness.build(tx, CancellationToken::new());

        let result = ingestor.ingest_blocks().await;
        assert!(matches!(result, Err(DataSourceError::BlockNotFound(_))));
    }

    #[tokio::test]
    async fn test_ingestor_starts_from_calculated_block_on_first_run() {
        let mut harness = TestHarness::new();
        // No last processed block in state
        harness.mock_state_repo.expect_get_last_processed_block().returning(|_| Ok(None));
        // Chain head is at 200
        harness.mock_data_source.expect_get_current_block_number().returning(|| Ok(200));
        // We expect it to start fetching from 100 (200 - 100)
        harness
            .mock_data_source
            .expect_fetch_block_core_data()
            .with(eq(100)) // Assert we start from the correct calculated block
            .times(1)
            .returning(|n| Ok((BlockBuilder::new().number(n).build(), vec![])));
        harness.mock_filtering_engine.expect_requires_receipt_data().returning(|| false);

        let (tx, _rx) = mpsc::channel(10);
        let ingestor = harness.build(tx, CancellationToken::new());

        // We only care about the first block fetch, so we ignore the result.
        let _ = ingestor.ingest_blocks().await;
    }

    #[tokio::test]
    async fn test_ingestor_stops_before_fetching_on_cancellation() {
        let mut harness = TestHarness::new();
        harness.mock_state_repo.expect_get_last_processed_block().returning(|_| Ok(Some(100)));
        harness.mock_data_source.expect_get_current_block_number().returning(|| Ok(120));

        // We expect no block fetches since we cancel before starting
        harness.mock_data_source.expect_fetch_block_core_data().times(0);

        let (tx, _rx) = mpsc::channel(10);
        let token = CancellationToken::new();

        // Cancel the token before calling the function
        token.cancel();

        let ingestor = harness.build(tx, token);

        let result = ingestor.ingest_blocks().await;
        assert!(result.is_ok(), "Should return Ok even if cancelled before starting");
    }

    #[tokio::test]
    async fn test_ingestor_run_loop_stops_on_cancellation() {
        use mockall::Sequence;

        let mut harness = TestHarness::new();
        harness.config = Arc::new(
            AppConfig::builder()
                .network_id(TEST_NETWORK_ID)
                .confirmation_blocks(1)
                .polling_interval(10) // Fast polling
                .build(),
        );

        let mut seq = Sequence::new();

        // --- Expectations for ONLY ONE successful run loop iteration ---
        harness
            .mock_state_repo
            .expect_get_last_processed_block()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Ok(Some(100)));
        harness
            .mock_data_source
            .expect_get_current_block_number()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Ok(102));
        harness
            .mock_data_source
            .expect_fetch_block_core_data()
            .with(eq(101))
            .times(1)
            .in_sequence(&mut seq)
            .returning(|n| Ok((BlockBuilder::new().number(n).build(), vec![])));
        harness
            .mock_filtering_engine
            .expect_requires_receipt_data()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| false);

        let (tx, mut rx) = mpsc::channel(10);
        let token = CancellationToken::new();

        let ingestor = harness.build(tx, token.clone());
        let ingestor_handle = tokio::spawn(ingestor.run());

        // Wait for the first block to be processed and sent, confirming the first loop
        // ran.
        let received_block = rx.recv().await.expect("Should receive one block");
        assert_eq!(received_block.block.header.number, 101);

        // Signal the shutdown.
        token.cancel();

        // The run() loop should now exit gracefully.
        match tokio::time::timeout(std::time::Duration::from_secs(1), ingestor_handle).await {
            Ok(Ok(_)) => { /* Task completed successfully, which is what we want */ }
            Ok(Err(e)) => panic!("Ingestor task panicked: {:?}", e),
            Err(_) => panic!("Ingestor task did not shut down within the timeout"),
        }
    }
}
