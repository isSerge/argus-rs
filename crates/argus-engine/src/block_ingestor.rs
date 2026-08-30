//! The BlockIngestor module is responsible for continuously fetching block data
//! from a DataSource and ingesting it into the processing pipeline.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use argus_core::{
    config::AppConfig,
    metrics::AppMetrics,
    models::BlockData,
    persistence::traits::AppRepository,
    providers::traits::{DataSource, DataSourceError},
};
use argus_providers::block_fetcher;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    filtering::FilteringEngine,
    polling::{BlockTimeCalibrator, live_poll_interval},
};

/// Default lookback (in blocks) on first run when no processed-block state
/// exists yet.
const DEFAULT_BACKFILL_BLOCKS: u64 = 100;

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
    /// Next block to fetch given what has already been sent downstream, which
    /// can run ahead of the persisted `last_processed_block` while the
    /// `BlockProcessor` works through the channel backlog.
    ///
    /// Invariant: if the persisted state ever moves backwards (e.g. future
    /// reorg handling), this high-water mark must be reset accordingly,
    /// otherwise re-disputed blocks would be skipped.
    next_block: AtomicU64,
    /// Warns when the observed chain block rate diverges from the configured
    /// `expected_block_time_ms`.
    calibrator: Mutex<BlockTimeCalibrator>,
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
        let expected_block_time = config.expected_block_time_ms;
        Self {
            config,
            state,
            app_metrics,
            data_source,
            filtering,
            raw_blocks_tx,
            cancellation_token,
            next_block: AtomicU64::new(0),
            calibrator: Mutex::new(BlockTimeCalibrator::new(expected_block_time)),
        }
    }

    /// Starts the long-running service loop.
    ///
    /// Each cycle drains to the chain head — fetching consecutive chunks
    /// back-to-back with no sleep in between, so catch-up throughput is not
    /// capped by the polling interval — and only then waits before the next
    /// poll: ~the chain's expected block time when configured and caught up,
    /// or `polling_interval_ms` as a backoff after errors. The bounded
    /// `raw_blocks` channel provides natural backpressure, letting the
    /// processor correlate chunk N while chunk N+1 is being fetched.
    pub async fn run(self) {
        loop {
            let backoff = match self.ingest_blocks().await {
                Ok(()) => false,
                Err(e) => {
                    tracing::error!(
                        error = %e,
                        "Error during block ingestion cycle. Retrying after delay..."
                    );
                    true
                }
            };

            let polling_delay = tokio::time::sleep(self.next_poll_delay(backoff));

            tokio::select! {
                biased;

                _ = self.cancellation_token.cancelled() => {
                    tracing::info!("BlockIngestor cancellation signal received, shutting down...");
                    break;
                }

                _ = polling_delay => {}
            }
        }
        tracing::info!("BlockIngestor has shut down.");
    }

    /// Delay before the next poll: `polling_interval_ms` after errors,
    /// otherwise the live cadence (which tracks the chain's block time when
    /// configured).
    fn next_poll_delay(&self, backoff: bool) -> Duration {
        if backoff { self.config.polling_interval_ms } else { live_poll_interval(&self.config) }
    }

    /// Performs one ingestion cycle: drains consecutive block chunks until
    /// the safe head is reached, sending each chunk downstream.
    async fn ingest_blocks(&self) -> Result<(), DataSourceError> {
        let network_id = &self.config.network_id;
        let needs_receipts = self.filtering.requires_receipt_data();
        let last_processed_block = self.state.get_last_processed_block(network_id).await?;
        let current_block = self.data_source.get_current_block_number().await?;
        self.observe_block_time(current_block);

        if current_block < self.config.confirmation_blocks {
            tracing::debug!(
                "Chain is shorter than the confirmation buffer. Waiting for more blocks."
            );
            return Ok(());
        }

        let safe_to_block = current_block.saturating_sub(self.config.confirmation_blocks);
        let state_next = last_processed_block.map_or_else(
            || current_block.saturating_sub(DEFAULT_BACKFILL_BLOCKS),
            |block| block + 1,
        );
        let mut from_block = state_next.max(self.next_block.load(Ordering::Acquire));

        if from_block > safe_to_block {
            tracing::debug!("Caught up to confirmation buffer. Waiting for more blocks.");
            return Ok(());
        }

        tracing::info!(
            from_block = from_block,
            to_block = safe_to_block,
            "Draining block range to head."
        );

        let fetch_config = block_fetcher::FetchConfig::new(
            self.config.concurrency as usize,
            self.config.log_chunk_size,
        );

        while from_block <= safe_to_block {
            if self.cancellation_token.is_cancelled() {
                tracing::info!("Cancellation requested, stopping block ingestion.");
                return Ok(());
            }

            let to_block = std::cmp::min(from_block + self.config.block_chunk_size, safe_to_block);
            tracing::debug!(from_block = from_block, to_block = to_block, "Fetching block chunk.");

            // Use the same batch processing approach as dry_run for consistency
            let block_data_batch = block_fetcher::fetch_blocks_concurrent(
                self.data_source.as_ref(),
                needs_receipts,
                from_block,
                to_block,
                fetch_config,
            )
            .await?;

            // Send each block in the successfully fetched batch
            for block_data in block_data_batch {
                if self.cancellation_token.is_cancelled() {
                    tracing::info!("Cancellation requested, stopping block ingestion.");
                    return Ok(());
                }

                let block_timestamp = block_data.block.header.timestamp;
                let block_num = block_data.block.header.number;

                if self.raw_blocks_tx.send(block_data).await.is_err() {
                    tracing::warn!("Raw blocks channel closed, stopping further ingestion.");
                    return Err(DataSourceError::ChannelClosed);
                }

                self.next_block.store(block_num + 1, Ordering::Release);

                let mut metrics = self.app_metrics.metrics.write().await;
                metrics.latest_processed_block = block_num;
                metrics.latest_processed_block_timestamp_secs = block_timestamp;
            }

            from_block = to_block + 1;
        }

        Ok(())
    }

    fn observe_block_time(&self, head: u64) {
        if let Ok(mut calibrator) = self.calibrator.lock() {
            calibrator.observe(Instant::now(), head);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use alloy::primitives::B256;
    use argus_core::{
        models::NetworkId,
        persistence::traits::MockAppRepository,
        providers::traits::MockDataSource,
        test_utils::{BlockBuilder, ReceiptBuilder, TransactionBuilder},
    };
    use mockall::predicate::{always, eq};

    use super::*;
    use crate::filtering::MockFilteringEngine;

    fn spawn_collector(mut rx: mpsc::Receiver<BlockData>) -> tokio::task::JoinHandle<Vec<u64>> {
        tokio::spawn(async move {
            let mut nums = Vec::new();
            while let Some(block_data) = rx.recv().await {
                nums.push(block_data.block.header.number);
            }
            nums
        })
    }

    struct TestHarness {
        config: Arc<AppConfig>,
        mock_state_repo: MockAppRepository,
        mock_data_source: MockDataSource,
        mock_filtering_engine: MockFilteringEngine,
    }

    impl TestHarness {
        fn new() -> Self {
            let config = Arc::new(
                AppConfig::builder()
                    .network_id(&NetworkId::default())
                    .confirmation_blocks(1)
                    .concurrency(1) // Use concurrency of 1 for tests to avoid mock issues
                    .build(),
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

        // Set up the mocks in order they'll be called
        harness.mock_filtering_engine.expect_requires_receipt_data().times(1).returning(|| false);

        harness
            .mock_state_repo
            .expect_get_last_processed_block()
            .times(1)
            .returning(|_| Ok(Some(121)));

        harness.mock_data_source.expect_get_current_block_number().times(1).returning(|| Ok(123));

        // The concurrent batch will call fetch_block_only + fetch_logs_for_range for
        // block 122
        harness
            .mock_data_source
            .expect_fetch_block_only()
            .with(eq(122))
            .times(1)
            .returning(|block_num| Ok(BlockBuilder::new().number(block_num).build()));
        harness.mock_data_source.expect_fetch_logs_for_range().returning(|_, _| Ok(vec![]));

        // Since receipts are not required, fetch_receipts should not be called
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
            .expect_fetch_block_only()
            .with(eq(122))
            .returning(|block_num| Ok(BlockBuilder::new().number(block_num).build()));
        harness.mock_data_source.expect_fetch_logs_for_range().returning(|_, _| Ok(vec![]));
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
            .expect_fetch_block_only()
            .with(eq(122))
            .returning(move |block_num| Ok(block.clone().number(block_num).build()));
        harness.mock_data_source.expect_fetch_logs_for_range().returning(|_, _| Ok(vec![]));
        harness
            .mock_data_source
            .expect_fetch_receipts()
            .with(eq(vec![tx_hash]), always())
            .returning(move |_, _| Ok(expected_receipts.clone()));

        let (tx, _rx) = mpsc::channel(10);
        let ingestor = harness.build(tx, CancellationToken::new());

        let result = ingestor.ingest_blocks().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_ingestor_waits_when_caught_up_to_confirmation_head() {
        // This test ensures that if `from_block` > `safe_to_block`, we fetch nothing.
        let mut harness = TestHarness::new();

        harness.mock_filtering_engine.expect_requires_receipt_data().times(1).returning(|| false);

        // last processed is 121, next should be 122
        harness.mock_state_repo.expect_get_last_processed_block().returning(|_| Ok(Some(121)));
        // current is 122, so safe_to_block is 121
        harness.mock_data_source.expect_get_current_block_number().returning(|| Ok(122));
        // We should not attempt to fetch any block data
        harness.mock_data_source.expect_fetch_block_only().times(0);
        harness.mock_data_source.expect_fetch_logs_for_range().times(0);

        let (tx, _rx) = mpsc::channel(10);
        let ingestor = harness.build(tx, CancellationToken::new());

        let result = ingestor.ingest_blocks().await;
        assert!(result.is_ok(), "Should return Ok(()) even if no blocks are processed");
    }

    #[tokio::test]
    async fn test_ingestor_handles_data_source_error_gracefully() {
        let mut harness = TestHarness::new();

        harness.mock_filtering_engine.expect_requires_receipt_data().times(1).returning(|| false);

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
    async fn test_ingest_blocks_drains_to_head_in_one_cycle() {
        let mut harness = TestHarness::new();
        harness.config = Arc::new(
            AppConfig::builder()
                .network_id(&NetworkId::default())
                .confirmation_blocks(1)
                .concurrency(1)
                .block_chunk_size(4)
                .build(),
        );

        harness.mock_filtering_engine.expect_requires_receipt_data().returning(|| false);
        harness.mock_state_repo.expect_get_last_processed_block().returning(|_| Ok(Some(100)));
        // The head is fetched once per cycle; draining 101..=109 requires
        // multiple chunks, proving they are fetched back-to-back.
        harness.mock_data_source.expect_get_current_block_number().times(1).returning(|| Ok(110));

        let fetched = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&fetched);
        harness.mock_data_source.expect_fetch_block_only().returning(move |n| {
            recorded.lock().unwrap().push(n);
            Ok(BlockBuilder::new().number(n).build())
        });
        harness.mock_data_source.expect_fetch_logs_for_range().returning(|_, _| Ok(vec![]));

        let (tx, rx) = mpsc::channel(64);
        let collector = spawn_collector(rx);
        let ingestor = harness.build(tx, CancellationToken::new());

        ingestor.ingest_blocks().await.unwrap();
        drop(ingestor);

        let nums = collector.await.unwrap();
        assert_eq!(nums, (101..=109).collect::<Vec<_>>());
        assert_eq!(*fetched.lock().unwrap(), nums);
    }

    #[tokio::test]
    async fn test_ingestor_does_not_refetch_blocks_already_sent_downstream() {
        let mut harness = TestHarness::new();
        harness.config = Arc::new(
            AppConfig::builder()
                .network_id(&NetworkId::default())
                .confirmation_blocks(1)
                .concurrency(1)
                .block_chunk_size(50)
                .build(),
        );

        harness.mock_filtering_engine.expect_requires_receipt_data().returning(|| false);
        // The persisted state lags behind: it still reports 100 even after
        // blocks have been sent downstream (the BlockProcessor has not
        // committed yet).
        harness.mock_state_repo.expect_get_last_processed_block().returning(|_| Ok(Some(100)));

        let head = AtomicU64::new(102);
        harness
            .mock_data_source
            .expect_get_current_block_number()
            .times(2)
            .returning(move || Ok(head.fetch_add(1, Ordering::Relaxed)));

        let fetched = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&fetched);
        harness.mock_data_source.expect_fetch_block_only().returning(move |n| {
            recorded.lock().unwrap().push(n);
            Ok(BlockBuilder::new().number(n).build())
        });
        harness.mock_data_source.expect_fetch_logs_for_range().returning(|_, _| Ok(vec![]));

        let (tx, rx) = mpsc::channel(64);
        let collector = spawn_collector(rx);
        let ingestor = harness.build(tx, CancellationToken::new());

        // Cycle 1: head 102, safe head 101 -> sends 101.
        ingestor.ingest_blocks().await.unwrap();
        // Cycle 2: head 103, safe head 102 -> must send only 102, not re-send 101.
        ingestor.ingest_blocks().await.unwrap();
        drop(ingestor);

        let nums = collector.await.unwrap();
        assert_eq!(nums, vec![101, 102]);
        assert_eq!(*fetched.lock().unwrap(), nums);
    }

    #[tokio::test]
    async fn test_ingestor_starts_from_calculated_block_on_first_run() {
        let mut harness = TestHarness::new();
        harness.config = Arc::new(
            AppConfig::builder()
                .network_id(&NetworkId::default())
                .confirmation_blocks(1)
                .concurrency(1)
                .block_chunk_size(200)
                .build(),
        );

        // No last processed block in state; chain head is at 200.
        harness.mock_state_repo.expect_get_last_processed_block().returning(|_| Ok(None));
        harness.mock_data_source.expect_get_current_block_number().returning(|| Ok(200));

        let fetched = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&fetched);
        harness.mock_data_source.expect_fetch_block_only().returning(move |n| {
            recorded.lock().unwrap().push(n);
            Ok(BlockBuilder::new().number(n).build())
        });
        harness.mock_data_source.expect_fetch_logs_for_range().returning(|_, _| Ok(vec![]));
        harness.mock_filtering_engine.expect_requires_receipt_data().returning(|| false);

        let (tx, rx) = mpsc::channel(10);
        let collector = spawn_collector(rx);
        let ingestor = harness.build(tx, CancellationToken::new());

        ingestor.ingest_blocks().await.unwrap();
        drop(ingestor);

        // It should start from 100 (200 - 100) and drain to the safe head (199).
        let nums = collector.await.unwrap();
        assert_eq!(nums, (100..=199).collect::<Vec<_>>());
        assert_eq!(*fetched.lock().unwrap(), nums);
    }

    #[tokio::test]
    async fn test_ingestor_stops_before_fetching_on_cancellation() {
        let mut harness = TestHarness::new();

        harness.mock_filtering_engine.expect_requires_receipt_data().times(1).returning(|| false);

        harness.mock_state_repo.expect_get_last_processed_block().returning(|_| Ok(Some(100)));

        harness.mock_data_source.expect_get_current_block_number().returning(|| Ok(120));

        // Once cancelled, the ingestor should stop before fetching anything.
        harness.mock_data_source.expect_fetch_block_only().times(0);
        harness.mock_data_source.expect_fetch_logs_for_range().times(0);

        let (tx, _rx) = mpsc::channel(10);
        let token = CancellationToken::new();

        // Cancel the token before calling the function
        token.cancel();

        let ingestor = harness.build(tx, token);

        let result = ingestor.ingest_blocks().await;
        assert!(result.is_ok(), "Should return Ok even if cancelled during processing");
    }

    #[tokio::test]
    async fn test_ingestor_run_loop_stops_on_cancellation() {
        use mockall::Sequence;

        let mut harness = TestHarness::new();
        harness.config = Arc::new(
            AppConfig::builder()
                .network_id(&NetworkId::default())
                .confirmation_blocks(1)
                .concurrency(1) // Use concurrency of 1 for tests
                .polling_interval(10) // Fast polling
                .build(),
        );

        let mut seq = Sequence::new();

        // Expectations for one successful run loop iteration - in correct order
        harness
            .mock_filtering_engine
            .expect_requires_receipt_data()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| false);
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
            .expect_fetch_block_only()
            .with(eq(101))
            .times(1)
            .in_sequence(&mut seq)
            .returning(|n| Ok(BlockBuilder::new().number(n).build()));
        // fetch_logs_for_range runs concurrently with fetch_block_only via tokio::join!
        harness.mock_data_source.expect_fetch_logs_for_range().returning(|_, _| Ok(vec![]));

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
