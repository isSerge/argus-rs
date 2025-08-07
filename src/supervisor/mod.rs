//! The Supervisor module manages the lifecycle of the Argus application, coordinating between the engine, data sources, and state repository.

mod builder;

use std::{collections::HashMap, sync::Arc, time::Duration};

use crate::{
    config::AppConfig,
    engine::{
        block_processor::{BlockProcessor, BlockProcessorError},
        filtering::FilteringEngine,
    },
    models::{BlockData, DecodedBlockData},
    persistence::traits::StateRepository,
    providers::traits::{DataSource, DataSourceError},
};
use builder::SupervisorBuilder;
use thiserror::Error;
use tokio::{signal, sync::mpsc};

/// SupervisorError represents errors that can occur within the Supervisor.
#[derive(Debug, Error)]
pub enum SupervisorError {
    /// Error indicating that the Supervisor is missing a configuration.
    #[error("Missing configuration for Supervisor")]
    MissingConfig,

    /// Error indicating that the Supervisor is missing a state repository.
    #[error("Missing state repository for Supervisor")]
    MissingStateRepository,

    /// Error indicating that the Supervisor is missing an ABI service.
    #[error("Missing ABI service for Supervisor")]
    MissingAbiService,

    /// Error indicating that the Supervisor is missing a data source.
    #[error("Missing data source for Supervisor")]
    MissingDataSource,

    // TODO: convert sqlx error to RepositoryError
    /// Error indicating that the Supervisor encountered an issue while loading monitors from state repository.
    #[error("Failed to load monitors from state repository: {0}")]
    MonitorLoadError(#[from] sqlx::Error),

    /// Error indicating that the Supervisor encountered an issue with the data source.
    #[error("Data source error: {0}")]
    DataSourceError(#[from] DataSourceError),

    /// Error indicating that the Supervisor encountered an issue with the channel.
    #[error("Channel closed")]
    ChannelClosed,
}

/// The Supervisor is responsible for managing the application state, processing blocks, and applying filters.
pub struct Supervisor {
    /// The configuration for the Supervisor
    config: AppConfig,
    /// The state repository for managing application state.
    state: Arc<dyn StateRepository>,
    /// The data source for fetching blockchain data.
    data_source: Box<dyn DataSource>,
    /// The block processor for processing blockchain data.
    processor: BlockProcessor,
    /// The filtering engine for applying filters to the processed data.
    filtering: Arc<dyn FilteringEngine>,
    /// A cancellation token for gracefully shutting down the Supervisor.
    cancellation_token: tokio_util::sync::CancellationToken,
    /// A set of tasks that the Supervisor is managing.
    join_set: tokio::task::JoinSet<()>,
}

impl Supervisor {
    /// Creates a new Supervisor instance with the provided configuration and components.
    pub fn new(
        config: AppConfig,
        state: Arc<dyn StateRepository>,
        data_source: Box<dyn DataSource>,
        processor: BlockProcessor,
        filtering: Arc<dyn FilteringEngine>,
    ) -> Self {
        Self {
            config,
            state,
            data_source,
            processor,
            filtering,
            cancellation_token: tokio_util::sync::CancellationToken::new(),
            join_set: tokio::task::JoinSet::new(),
        }
    }

    /// Starts the Supervisor, initializing all components and beginning the processing loop.
    pub async fn run(mut self) -> Result<(), SupervisorError> {
        // Initialize the cancellation token
        let cancellation_token = self.cancellation_token.clone();

        // Spawn a task to listen for cancellation signals (e.g., Ctrl+C)
        self.join_set.spawn(async move {
            let ctrl_c = signal::ctrl_c();
            #[cfg(unix)]
            let terminate = async {
                signal::unix::signal(signal::unix::SignalKind::terminate())
                    .expect("Failed to register SIGTERM handler")
                    .recv()
                    .await;
            };
            #[cfg(not(unix))]
            let terminate = std::future::pending::<()>();

            tokio::select! {
                _ = ctrl_c => tracing::info!("SIGINT (Ctrl+C) received, initiating graceful shutdown."),
                _ = terminate => tracing::info!("SIGTERM received, initiating graceful shutdown."),
            }

            // Cancel the cancellation token to signal shutdown
            cancellation_token.cancel();
        });

        // Create a channel for decoded block data
        let (decoded_blocks_tx, decoded_blocks_rx) =
            mpsc::channel::<DecodedBlockData>(self.config.block_chunk_size as usize * 2);

        // Spawn the filtering engine task
        let filtering_engine_clone = Arc::clone(&self.filtering);
        self.join_set.spawn(async move {
            filtering_engine_clone.run(decoded_blocks_rx).await;
        });

        // TODO: spawn other tasks similar to the filtering engine

        loop {
            let tx_clone = decoded_blocks_tx.clone();
            let polling_delay =
                tokio::time::sleep(Duration::from_millis(self.config.polling_interval_ms));

            tokio::select! {
              biased;

              // Cancellation token branch
              _ = self.cancellation_token.cancelled() => {
                tracing::info!("Supervisor cancellation signal received, shutting down...");
                break;
              }

              // Check if any spawned tasks failed
              Some(result) = self.join_set.join_next() => {
                if let Err(e) = result {
                  tracing::error!("Task failed: {:?}", e);
                  self.cancellation_token.cancel();
                }

                continue;
              }

              // Monitor cycle branch
              _ = polling_delay => {
                if let Err(e) = self.monitor_cycle(tx_clone).await {
                    tracing::error!(error = %e, "Error in monitoring cycle. Retrying after delay...");
                }
              }
            }
        }

        // Graceful shutdown of spawned tasks
        self.join_set.shutdown().await;
        tracing::info!("All tasks completed, shutting down Supervisor...");

        // Perform cleanup operations
        tracing::info!("Starting graceful cleanup...");

        let shutdown_timeout = Duration::from_secs(self.config.shutdown_timeout_secs);

        let cleanup_logic = async {
            if let Err(e) = self.state.flush().await {
                tracing::error!(error = %e, "Failed to flush pending writes, but continuing cleanup.");
            }
            if let Err(e) = self.state.cleanup().await {
                tracing::error!(error = %e, "Failed to perform state repository cleanup, but continuing.");
            }
            match self
                .state
                .get_last_processed_block(&self.config.network_id)
                .await
            {
                Ok(Some(last_block)) => tracing::info!(
                    last_processed_block = last_block,
                    "Final state: last processed block recorded."
                ),
                Ok(None) => tracing::info!("Final state: no blocks have been processed yet."),
                Err(e) => {
                    tracing::warn!(error = %e, "Could not retrieve final state during cleanup.")
                }
            }
        };

        match tokio::time::timeout(shutdown_timeout, cleanup_logic).await {
            Ok(_) => tracing::info!("Cleanup completed successfully."),
            Err(_) => tracing::warn!(
                "Cleanup did not complete within the timeout of {:?}. Continuing shutdown.",
                shutdown_timeout
            ),
        }

        tracing::info!("Supervisor shutdown complete.");
        Ok(())
    }

    async fn monitor_cycle(
        &self,
        decoded_blocks_tx: mpsc::Sender<DecodedBlockData>,
    ) -> Result<(), SupervisorError> {
        let network_id = &self.config.network_id;
        let last_processed_block = self.state.get_last_processed_block(network_id).await?;
        let current_block = self.data_source.get_current_block_number().await?;

        if current_block < self.config.confirmation_blocks {
            tracing::info!(
                "Chain is shorter than the confirmation buffer. Waiting for more blocks."
            );
            return Ok(());
        }

        let from_block = last_processed_block
            .map_or_else(|| current_block.saturating_sub(100), |block| block + 1);
        let safe_to_block = current_block.saturating_sub(self.config.confirmation_blocks);

        if from_block > safe_to_block {
            tracing::info!("Caught up to confirmation buffer. Waiting for more blocks.");
            return Ok(());
        }

        let to_block = std::cmp::min(from_block + self.config.block_chunk_size, safe_to_block);
        tracing::info!(
            from_block = from_block,
            to_block = to_block,
            "Processing block range."
        );

        let mut last_processed = last_processed_block;
        let mut blocks_to_process = Vec::new();
        for block_num in from_block..=to_block {
            // Check if cancellation is requested
            if self.cancellation_token.is_cancelled() {
                tracing::info!("Cancellation requested, stopping block processing.");
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
            blocks_to_process.push(BlockData::from_raw_data(block, receipts, logs));
        }

        if !blocks_to_process.is_empty() {
            match self.processor.process_blocks_batch(blocks_to_process).await {
                Ok(decoded_blocks) => {
                    for decoded_block in decoded_blocks {
                        let block_num = decoded_block.block_number;
                        if decoded_blocks_tx.send(decoded_block).await.is_err() {
                            tracing::warn!(
                                "Decoded blocks channel closed, stopping further processing."
                            );
                            return Err(SupervisorError::ChannelClosed);
                        }
                        last_processed = Some(block_num);
                    }
                }
                Err(BlockProcessorError::AbiService(e)) => {
                    tracing::warn!(error = %e, "ABI service error during batch processing, will retry.");
                }
            }
        }

        if let Some(valid_last_processed) = last_processed
            && Some(valid_last_processed) > last_processed_block
        {
            self.state
                .set_last_processed_block(network_id, valid_last_processed)
                .await?;
            tracing::info!(
                last_processed_block = valid_last_processed,
                "Last processed block updated successfully."
            );
        }

        Ok(())
    }

    /// Creates a new SupervisorBuilder to configure and build a Supervisor instance.
    pub fn builder() -> SupervisorBuilder {
        SupervisorBuilder::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        abi::AbiService,
        engine::filtering::MockFilteringEngine,
        persistence::traits::MockStateRepository,
        providers::traits::MockDataSource,
        test_helpers::{BlockBuilder, ReceiptBuilder, TransactionBuilder},
    };
    use alloy::primitives::B256;
    use mockall::predicate::eq;
    use std::{collections::HashMap, sync::Arc};

    /// A test harness to simplify supervisor testing by holding common mocks.
    struct SupervisorTestHarness {
        config: AppConfig,
        mock_state_repo: MockStateRepository,
        mock_data_source: MockDataSource,
        mock_filtering_engine: MockFilteringEngine,
        block_processor: BlockProcessor,
    }

    impl SupervisorTestHarness {
        fn new() -> Self {
            let mut config = AppConfig::default();
            config.confirmation_blocks = 1;

            let abi_service = Arc::new(AbiService::new());
            let block_processor = BlockProcessor::new(abi_service);

            Self {
                config,
                mock_state_repo: MockStateRepository::new(),
                mock_data_source: MockDataSource::new(),
                mock_filtering_engine: MockFilteringEngine::new(),
                block_processor,
            }
        }

        fn build(self) -> Supervisor {
            Supervisor::new(
                self.config,
                Arc::new(self.mock_state_repo),
                Box::new(self.mock_data_source),
                self.block_processor,
                Arc::new(self.mock_filtering_engine),
            )
        }
    }

    #[tokio::test]
    async fn test_monitor_cycle_succeeds_without_fetching_receipts_when_not_required() {
        // Arrange
        let mut harness = SupervisorTestHarness::new();

        harness
            .mock_filtering_engine
            .expect_requires_receipt_data()
            .returning(|| false);
        harness
            .mock_state_repo
            .expect_get_last_processed_block()
            .returning(|_| Ok(Some(121)));
        harness
            .mock_data_source
            .expect_get_current_block_number()
            .returning(|| Ok(123));
        harness
            .mock_data_source
            .expect_fetch_block_core_data()
            .with(eq(122))
            .returning(|block_num| Ok((BlockBuilder::new().number(block_num).build(), vec![])));
        harness.mock_data_source.expect_fetch_receipts().times(0);
        harness
            .mock_state_repo
            .expect_set_last_processed_block()
            .returning(|_, _| Ok(()));

        let supervisor = harness.build();
        let (tx, mut rx) = mpsc::channel(10);
        let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });

        // Act
        let result = supervisor.monitor_cycle(tx).await;

        // Assert
        assert!(result.is_ok());
        drain.abort();
    }

    #[tokio::test]
    async fn test_monitor_cycle_skips_receipt_fetch_for_empty_block() {
        // Arrange
        let mut harness = SupervisorTestHarness::new();

        harness
            .mock_filtering_engine
            .expect_requires_receipt_data()
            .returning(|| true);
        harness
            .mock_state_repo
            .expect_get_last_processed_block()
            .returning(|_| Ok(Some(121)));
        harness
            .mock_data_source
            .expect_get_current_block_number()
            .returning(|| Ok(123));
        harness
            .mock_data_source
            .expect_fetch_block_core_data()
            .with(eq(122))
            .returning(|block_num| Ok((BlockBuilder::new().number(block_num).build(), vec![])));
        harness.mock_data_source.expect_fetch_receipts().times(0);
        harness
            .mock_state_repo
            .expect_set_last_processed_block()
            .returning(|_, _| Ok(()));

        let supervisor = harness.build();
        let (tx, mut rx) = mpsc::channel(10);
        let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });

        // Act
        let result = supervisor.monitor_cycle(tx).await;

        // Assert
        assert!(result.is_ok());
        drain.abort();
    }

    #[tokio::test]
    async fn test_monitor_cycle_fetches_receipts_successfully_when_required() {
        // Arrange
        let mut harness = SupervisorTestHarness::new();
        let tx_hash = B256::from([1u8; 32]);
        let block =
            BlockBuilder::new().transaction(TransactionBuilder::new().hash(tx_hash).build());
        let receipt = ReceiptBuilder::new().transaction_hash(tx_hash).build();
        let mut expected_receipts = HashMap::new();
        expected_receipts.insert(tx_hash, receipt);

        harness
            .mock_filtering_engine
            .expect_requires_receipt_data()
            .returning(|| true);
        harness
            .mock_state_repo
            .expect_get_last_processed_block()
            .returning(|_| Ok(Some(121)));
        harness
            .mock_data_source
            .expect_get_current_block_number()
            .returning(|| Ok(123));
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
        harness
            .mock_state_repo
            .expect_set_last_processed_block()
            .returning(|_, _| Ok(()));

        let supervisor = harness.build();
        let (tx, mut rx) = mpsc::channel(10);
        let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });

        // Act
        let result = supervisor.monitor_cycle(tx).await;

        // Assert
        assert!(result.is_ok());
        drain.abort();
    }

    #[tokio::test]
    async fn test_monitor_cycle_fails_when_receipt_fetch_fails() {
        // Arrange
        let mut harness = SupervisorTestHarness::new();
        let tx_hash = B256::from([1u8; 32]);
        let block =
            BlockBuilder::new().transaction(TransactionBuilder::new().hash(tx_hash).build());

        harness
            .mock_filtering_engine
            .expect_requires_receipt_data()
            .returning(|| true);
        harness
            .mock_state_repo
            .expect_get_last_processed_block()
            .returning(|_| Ok(Some(121)));
        harness
            .mock_data_source
            .expect_get_current_block_number()
            .returning(|| Ok(123));
        harness
            .mock_data_source
            .expect_fetch_block_core_data()
            .with(eq(122))
            .returning(move |block_num| Ok((block.clone().number(block_num).build(), vec![])));
        harness
            .mock_data_source
            .expect_fetch_receipts()
            .with(eq(vec![tx_hash]))
            .returning(|_| {
                Err(DataSourceError::Provider(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "RPC error",
                ))))
            });
        harness
            .mock_state_repo
            .expect_set_last_processed_block()
            .times(0);

        let supervisor = harness.build();
        let (tx, mut rx) = mpsc::channel(10);
        let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });

        // Act
        let result = supervisor.monitor_cycle(tx).await;

        // Assert
        assert!(result.is_err());
        drain.abort();
    }

    #[tokio::test]
    async fn test_monitor_cycle_handles_multiple_transactions_successfully() {
        // Arrange
        let mut harness = SupervisorTestHarness::new();
        let tx_hash1 = B256::from([1u8; 32]);
        let tx_hash2 = B256::from([2u8; 32]);
        let block = BlockBuilder::new()
            .transaction(TransactionBuilder::new().hash(tx_hash1).build())
            .transaction(TransactionBuilder::new().hash(tx_hash2).build());

        let mut expected_receipts = HashMap::new();
        expected_receipts.insert(
            tx_hash1,
            ReceiptBuilder::new().transaction_hash(tx_hash1).build(),
        );
        expected_receipts.insert(
            tx_hash2,
            ReceiptBuilder::new().transaction_hash(tx_hash2).build(),
        );

        harness
            .mock_filtering_engine
            .expect_requires_receipt_data()
            .returning(|| true);
        harness
            .mock_state_repo
            .expect_get_last_processed_block()
            .returning(|_| Ok(Some(121)));
        harness
            .mock_data_source
            .expect_get_current_block_number()
            .returning(|| Ok(123));
        harness
            .mock_data_source
            .expect_fetch_block_core_data()
            .with(eq(122))
            .returning(move |block_num| Ok((block.clone().number(block_num).build(), vec![])));
        harness
            .mock_data_source
            .expect_fetch_receipts()
            .with(eq(vec![tx_hash1, tx_hash2]))
            .returning(move |_| Ok(expected_receipts.clone()));
        harness
            .mock_state_repo
            .expect_set_last_processed_block()
            .returning(|_, _| Ok(()));

        let supervisor = harness.build();
        let (tx, mut rx) = mpsc::channel(10);
        let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });

        // Act
        let result = supervisor.monitor_cycle(tx).await;

        // Assert
        assert!(result.is_ok());
        drain.abort();
    }
}
