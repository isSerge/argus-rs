//! The Supervisor module manages the lifecycle of the Argus application.
//!
//! This module implements the **Supervisor Pattern**, a design pattern used to
//! manage the lifecycle of multiple, concurrent, long-running services. It acts
//! as the top-level owner of all major components of the application, such as
//! the data source, block processor, and filtering engine.
//!
//! ## Responsibilities
//!
//! - **Initialization**: The `SupervisorBuilder` constructs and "wires" all
//!   services together, injecting necessary dependencies like configuration and
//!   database connections.
//! - **Lifecycle Management**: The `Supervisor` starts all services and manages
//!   their lifetimes.
//! - **Graceful Shutdown**: It listens for shutdown signals (like Ctrl+C or
//!   SIGTERM) and orchestrates a clean shutdown of all managed services.
//! - **Task Supervision**: It monitors the health of each service. If a
//!   critical service fails (panics or returns an error), the supervisor will
//!   shut down all other services to ensure the application exits cleanly
//!   rather than continuing in a partially-functional state.

mod builder;

use std::{collections::HashMap, sync::Arc};

use builder::SupervisorBuilder;
use thiserror::Error;
use tokio::{signal, sync::mpsc};

use crate::{
    config::AppConfig,
    engine::{
        alert_manager::AlertManager,
        block_processor::{BlockProcessor, BlockProcessorError},
        filtering::FilteringEngine,
    },
    models::{BlockData, DecodedBlockData, monitor_match::MonitorMatch},
    monitor::MonitorValidationError,
    persistence::{sqlite::SqliteStateRepository, traits::StateRepository},
    providers::traits::{DataSource, DataSourceError},
};

/// Represents the set of errors that can occur during the supervisor's
/// operation.
#[derive(Debug, Error)]
pub enum SupervisorError {
    /// A required configuration was not provided to the `SupervisorBuilder`.
    #[error("Missing configuration for Supervisor")]
    MissingConfig,

    /// A state repository was not provided to the `SupervisorBuilder`.
    #[error("Missing state repository for Supervisor")]
    MissingStateRepository,

    /// An ABI service was not provided to the `SupervisorBuilder`.
    #[error("Missing ABI service for Supervisor")]
    MissingAbiService,

    /// A notification service was not provided to the `SupervisorBuilder`.
    #[error("Missing notification service for Supervisor")]
    MissingNotificationService,

    /// A data source was not provided to the `SupervisorBuilder`.
    #[error("Missing data source for Supervisor")]
    MissingDataSource,

    /// An error occurred while trying to load monitors from the state
    /// repository.
    #[error("Failed to load monitors from state repository: {0}")]
    MonitorLoadError(#[from] sqlx::Error),

    /// A critical error occurred in the data source during block fetching.
    #[error("Data source error: {0}")]
    DataSourceError(#[from] DataSourceError),

    /// The channel for communicating with a downstream service was closed
    /// unexpectedly.
    #[error("Channel closed")]
    ChannelClosed,

    /// An error occurred during monitor validation.
    #[error("Monitor validation error: {0}")]
    MonitorValidationError(#[from] MonitorValidationError),

    /// An error occurred due to an invalid configuration.
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// A script compiler was not provided to the `SupervisorBuilder`.
    #[error("Missing script compiler for Supervisor")]
    MissingScriptCompiler,
}

/// The primary runtime manager for the application.
///
/// The Supervisor owns all the major components (services) and is responsible
/// for their startup, shutdown, and health monitoring. Once `run` is called, it
/// becomes
//  the main process loop for the entire application.
pub struct Supervisor {
    /// Shared application configuration.
    config: AppConfig,

    /// The persistent state repository for managing application state.
    state: Arc<dyn StateRepository>,

    /// The data source for fetching new blockchain data (e.g., from an RPC
    /// endpoint).
    data_source: Box<dyn DataSource>,

    /// The service responsible for decoding raw block data.
    processor: BlockProcessor,

    /// The service responsible for matching decoded data against user-defined
    /// monitors.
    filtering: Arc<dyn FilteringEngine>,

    /// The alert manager that handles sending alerts based on
    /// matched monitors.
    alert_manager: Arc<AlertManager<SqliteStateRepository>>,

    /// A token used to signal a graceful shutdown to all supervised tasks.
    cancellation_token: tokio_util::sync::CancellationToken,

    /// A set of all spawned tasks that the supervisor is actively managing.
    join_set: tokio::task::JoinSet<()>,
}

impl Supervisor {
    /// Creates a new Supervisor instance with all its required components.
    ///
    /// This is typically called by the `SupervisorBuilder` after it has
    /// assembled all the necessary dependencies.
    pub fn new(
        config: AppConfig,
        state: Arc<SqliteStateRepository>,
        data_source: Box<dyn DataSource>,
        processor: BlockProcessor,
        filtering: Arc<dyn FilteringEngine>,
        alert_manager: Arc<AlertManager<SqliteStateRepository>>,
    ) -> Self {
        Self {
            config,
            state,
            data_source,
            processor,
            filtering,
            alert_manager,
            cancellation_token: tokio_util::sync::CancellationToken::new(),
            join_set: tokio::task::JoinSet::new(),
        }
    }

    /// Starts the supervisor and all its managed services.
    ///
    /// This method is the main entry point for the application's runtime. It
    /// performs the following steps:
    /// 1. Spawns a signal handler to listen for `SIGINT` (Ctrl+C) and
    ///    `SIGTERM`.
    /// 2. Spawns the `FilteringEngine` as a long-running background task.
    /// 3. Enters the main `select!` loop, which concurrently:
    ///    - Listens for the shutdown signal.
    ///    - Monitors the health of all spawned tasks via the `JoinSet`.
    ///    - Periodically calls `monitor_cycle` to perform the main
    ///      block-fetching logic.
    /// 4. Upon shutdown, it waits for all tasks to complete and performs
    ///    graceful cleanup of resources like the database connection.
    pub async fn run(mut self) -> Result<(), SupervisorError> {
        // Clone the token for the signal handler task.
        let cancellation_token = self.cancellation_token.clone();

        // Spawn a task to listen for shutdown signals.
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

            // Notify all other tasks to begin shutting down.
            cancellation_token.cancel();
        });

        // Create the channel that connects the main logic (monitor_cycle) to the
        // filtering engine.
        let (decoded_blocks_tx, decoded_blocks_rx) =
            mpsc::channel::<DecodedBlockData>(self.config.block_chunk_size as usize * 2);

        // Create the channel that connects the filtering engine to the alert manager.
        let (monitor_matches_tx, mut monitor_matches_rx) =
            mpsc::channel::<MonitorMatch>(self.config.notification_channel_capacity as usize);

        // Spawn the filtering engine as a managed task.
        let filtering_engine_clone = Arc::clone(&self.filtering);
        self.join_set.spawn(async move {
            filtering_engine_clone.run(decoded_blocks_rx, monitor_matches_tx).await;
        });

        // Add the AlertManager's main processing loop.
        let alert_manager_clone = Arc::clone(&self.alert_manager);
        self.join_set.spawn(async move {
            while let Some(monitor_match) = monitor_matches_rx.recv().await {
                if let Err(e) = alert_manager_clone.process_match(&monitor_match).await {
                    tracing::error!(
                        "Failed to process monitor match for notifier '{}': {}",
                        monitor_match.notifier_name,
                        e
                    );
                }
            }
        });

        // Add the AlertManager's background aggregation dispatcher.
        let dispatcher_alert_manager = Arc::clone(&self.alert_manager);
        self.join_set.spawn(async move {
            // TODO: make this interval configurable in `AppConfig` later.
            let check_interval = self.config.aggregation_check_interval_secs;
            dispatcher_alert_manager.run_aggregation_dispatcher(check_interval).await;
        });

        // This is the main application loop.
        loop {
            let tx_clone = decoded_blocks_tx.clone();
            let polling_delay = tokio::time::sleep(self.config.polling_interval_ms);

            tokio::select! {
              // Use `biased` to ensure the shutdown signal is always checked first.
              biased;

              // Branch 1: A shutdown has been requested.
              _ = self.cancellation_token.cancelled() => {
                tracing::info!("Supervisor cancellation signal received, shutting down...");
                break; // Exit the main loop.
              }

              // Branch 2: A supervised task has terminated.
              Some(result) = self.join_set.join_next() => {
                if let Err(e) = result {
                  tracing::error!("A critical task failed: {:?}. Initiating shutdown.", e);
                  self.cancellation_token.cancel();
                }
                continue; // Check for shutdown or continue polling.
              }

              // Branch 3: The polling interval has elapsed, time for a new cycle.
              _ = polling_delay => {
                if let Err(e) = self.monitor_cycle(tx_clone).await {
                    tracing::error!(error = %e, "Error in monitoring cycle. Retrying after delay...");
                }
              }
            }
        }

        // --- Graceful Shutdown ---

        // Wait for all spawned tasks to complete.
        self.join_set.shutdown().await;
        tracing::info!("All supervised tasks have completed.");

        // Perform final cleanup of resources, with a timeout.
        tracing::info!("Starting graceful resource cleanup...");
        let shutdown_timeout = self.config.shutdown_timeout;

        let cleanup_logic = async {
            if let Err(e) = self.state.flush().await {
                tracing::error!(error = %e, "Failed to flush pending writes, but continuing cleanup.");
            }
            if let Err(e) = self.state.cleanup().await {
                tracing::error!(error = %e, "Failed to perform state repository cleanup, but continuing.");
            }
            match self.state.get_last_processed_block(&self.config.network_id).await {
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

        if tokio::time::timeout(shutdown_timeout, cleanup_logic).await.is_err() {
            tracing::warn!(
                "Cleanup did not complete within the timeout of {:?}. Continuing shutdown.",
                shutdown_timeout
            );
        } else {
            tracing::info!("Cleanup completed successfully.");
        }

        tracing::info!("Supervisor shutdown complete.");
        Ok(())
    }

    /// Performs one cycle of fetching, processing, and dispatching block data.
    ///
    /// This method encapsulates the core logic that was previously in
    /// `main.rs`. It determines the range of blocks to process based on the
    /// last known state, fetches the required data, processes it, and sends
    /// it to the filtering engine via the provided channel.
    async fn monitor_cycle(
        &self,
        decoded_blocks_tx: mpsc::Sender<DecodedBlockData>,
    ) -> Result<(), SupervisorError> {
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

        let mut last_processed = last_processed_block;
        let mut blocks_to_process = Vec::new();
        for block_num in from_block..=to_block {
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
                Ok(decoded_blocks) =>
                    for decoded_block in decoded_blocks {
                        let block_num = decoded_block.block_number;
                        if decoded_blocks_tx.send(decoded_block).await.is_err() {
                            tracing::warn!(
                                "Decoded blocks channel closed, stopping further processing."
                            );
                            return Err(SupervisorError::ChannelClosed);
                        }
                        last_processed = Some(block_num);
                    },
                Err(BlockProcessorError::AbiService(e)) => {
                    tracing::warn!(error = %e, "ABI service error during batch processing, will retry.");
                }
            }
        }

        if let Some(valid_last_processed) = last_processed
            && Some(valid_last_processed) > last_processed_block
        {
            self.state.set_last_processed_block(network_id, valid_last_processed).await?;
            tracing::info!(
                last_processed_block = valid_last_processed,
                "Last processed block updated successfully."
            );
        }

        Ok(())
    }

    /// Returns a new `SupervisorBuilder` instance.
    ///
    /// This is the public entry point for creating a supervisor.
    pub fn builder() -> SupervisorBuilder {
        SupervisorBuilder::new()
    }
}

/// This module contains the integration tests for the Supervisor.
#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use alloy::primitives::B256;
    use mockall::predicate::eq;
    use tempfile::tempdir;

    use super::*;
    use crate::{
        abi::{AbiRepository, AbiService},
        engine::filtering::MockFilteringEngine,
        http_client::HttpClientPool,
        notification::NotificationService,
        providers::traits::MockDataSource,
        test_helpers::{BlockBuilder, ReceiptBuilder, TransactionBuilder},
    };

    const TEST_NETWORK_ID: &str = "testnet";

    async fn setup_test_db() -> SqliteStateRepository {
        let repo = SqliteStateRepository::new("sqlite::memory:")
            .await
            .expect("Failed to connect to in-memory db");
        repo.run_migrations().await.expect("Failed to run migrations");
        repo
    }

    /// A test harness to simplify supervisor testing by holding common mocks.
    struct SupervisorTestHarness {
        config: AppConfig,
        state_repo: Arc<SqliteStateRepository>,
        mock_data_source: MockDataSource,
        mock_filtering_engine: MockFilteringEngine,
        block_processor: BlockProcessor,
        alert_manager: Arc<AlertManager<SqliteStateRepository>>,
    }

    impl SupervisorTestHarness {
        async fn new() -> Self {
            let config =
                AppConfig::builder().network_id(TEST_NETWORK_ID).confirmation_blocks(1).build();

            let state_repo = Arc::new(setup_test_db().await);
            let dir = tempdir().unwrap();
            let abi_repository = Arc::new(AbiRepository::new(dir.path()).unwrap());
            let abi_service = Arc::new(AbiService::new(Arc::clone(&abi_repository)));
            let block_processor = BlockProcessor::new(abi_service);
            let http_client_pool = Arc::new(HttpClientPool::new());
            let notification_service =
                Arc::new(NotificationService::new(Arc::new(HashMap::new()), http_client_pool));
            let alert_manager = Arc::new(AlertManager::new(
                Arc::clone(&notification_service),
                state_repo.clone(),
                Arc::new(HashMap::new()),
            ));

            Self {
                config,
                state_repo,
                mock_data_source: MockDataSource::new(),
                mock_filtering_engine: MockFilteringEngine::new(),
                block_processor,
                alert_manager,
            }
        }

        fn build(self) -> Supervisor {
            Supervisor::new(
                self.config,
                self.state_repo,
                Box::new(self.mock_data_source),
                self.block_processor,
                Arc::new(self.mock_filtering_engine),
                self.alert_manager,
            )
        }
    }

    #[tokio::test]
    async fn test_monitor_cycle_succeeds_without_fetching_receipts_when_not_required() {
        // Arrange
        let mut harness = SupervisorTestHarness::new().await;

        // This test verifies the scenario where no monitor requires transaction
        // receipts.
        harness.mock_filtering_engine.expect_requires_receipt_data().returning(|| false);
        // Set the last processed block to 121.
        let _ = harness.state_repo.set_last_processed_block(TEST_NETWORK_ID, 121).await;
        // Simulate the current chain head is at block 123.
        harness.mock_data_source.expect_get_current_block_number().returning(|| Ok(123));
        // We expect the cycle to process the next safe block, which is 122.
        harness
            .mock_data_source
            .expect_fetch_block_core_data()
            .with(eq(122))
            .returning(|block_num| Ok((BlockBuilder::new().number(block_num).build(), vec![])));
        // The core assertion of this test: fetch_receipts should never be called.
        harness.mock_data_source.expect_fetch_receipts().times(0);

        let supervisor = harness.build();
        let (tx, _rx) = mpsc::channel(10);

        // Act
        let result = supervisor.monitor_cycle(tx).await;

        // Assert
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_monitor_cycle_skips_receipt_fetch_for_empty_block() {
        // Arrange
        let mut harness = SupervisorTestHarness::new().await;

        // Even if monitors require receipts, the fetch should be skipped for blocks
        // with no transactions.
        harness.mock_filtering_engine.expect_requires_receipt_data().returning(|| true);
        // Set processing block to 121
        let _ = harness.state_repo.set_last_processed_block(TEST_NETWORK_ID, 121).await;
        harness.mock_data_source.expect_get_current_block_number().returning(|| Ok(123));
        // Return a block that has no transactions.
        harness
            .mock_data_source
            .expect_fetch_block_core_data()
            .with(eq(122))
            .returning(|block_num| Ok((BlockBuilder::new().number(block_num).build(), vec![])));
        // The core assertion: fetch_receipts is not called because there are no
        // transaction hashes.
        harness.mock_data_source.expect_fetch_receipts().times(0);

        let supervisor = harness.build();
        let (tx, _rx) = mpsc::channel(10);

        // Act
        let result = supervisor.monitor_cycle(tx).await;

        // Assert
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_monitor_cycle_fetches_receipts_successfully_when_required() {
        // Arrange
        let mut harness = SupervisorTestHarness::new().await;
        let tx_hash = B256::from([1u8; 32]);
        let block =
            BlockBuilder::new().transaction(TransactionBuilder::new().hash(tx_hash).build());
        let receipt = ReceiptBuilder::new().transaction_hash(tx_hash).build();
        let mut expected_receipts = HashMap::new();
        expected_receipts.insert(tx_hash, receipt);

        // Simulate that monitors require receipts.
        harness.mock_filtering_engine.expect_requires_receipt_data().returning(|| true);

        // Set the last processed block to 121.
        let _ = harness.state_repo.set_last_processed_block(TEST_NETWORK_ID, 121).await;
        harness.mock_data_source.expect_get_current_block_number().returning(|| Ok(123));
        // Return a block with one transaction.
        harness
            .mock_data_source
            .expect_fetch_block_core_data()
            .with(eq(122))
            .returning(move |block_num| Ok((block.clone().number(block_num).build(), vec![])));
        // The core assertion: fetch_receipts is called once with the correct
        // transaction hash.
        harness
            .mock_data_source
            .expect_fetch_receipts()
            .with(eq(vec![tx_hash]))
            .returning(move |_| Ok(expected_receipts.clone()));

        let supervisor = harness.build();
        let (tx, _rx) = mpsc::channel(10);

        // Act
        let result = supervisor.monitor_cycle(tx).await;

        // Assert
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_monitor_cycle_fails_when_receipt_fetch_fails() {
        // Arrange
        let mut harness = SupervisorTestHarness::new().await;
        let tx_hash = B256::from([1u8; 32]);
        let block =
            BlockBuilder::new().transaction(TransactionBuilder::new().hash(tx_hash).build());

        // Simulate that monitors require receipts.
        harness.mock_filtering_engine.expect_requires_receipt_data().returning(|| true);
        // Set the last processed block to 121.
        let _ = harness.state_repo.set_last_processed_block(TEST_NETWORK_ID, 121).await;
        harness.mock_data_source.expect_get_current_block_number().returning(|| Ok(123));
        harness
            .mock_data_source
            .expect_fetch_block_core_data()
            .with(eq(122))
            .returning(move |block_num| Ok((block.clone().number(block_num).build(), vec![])));
        // The core assertion: fetch_receipts is called, but it returns an error.
        harness.mock_data_source.expect_fetch_receipts().with(eq(vec![tx_hash])).returning(|_| {
            Err(DataSourceError::Provider(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                "RPC error",
            ))))
        });

        let supervisor = harness.build();
        let (tx, _rx) = mpsc::channel(10);

        // Act
        let result = supervisor.monitor_cycle(tx).await;

        // Assert
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_monitor_cycle_handles_multiple_transactions_successfully() {
        // Arrange
        let mut harness = SupervisorTestHarness::new().await;
        let tx_hash1 = B256::from([1u8; 32]);
        let tx_hash2 = B256::from([2u8; 32]);
        let block = BlockBuilder::new()
            .transaction(TransactionBuilder::new().hash(tx_hash1).build())
            .transaction(TransactionBuilder::new().hash(tx_hash2).build());

        let mut expected_receipts = HashMap::new();
        expected_receipts
            .insert(tx_hash1, ReceiptBuilder::new().transaction_hash(tx_hash1).build());
        expected_receipts
            .insert(tx_hash2, ReceiptBuilder::new().transaction_hash(tx_hash2).build());

        // Simulate that monitors require receipts.
        harness.mock_filtering_engine.expect_requires_receipt_data().returning(|| true);
        // Set the last processed block to 121.
        let _ = harness.state_repo.set_last_processed_block(TEST_NETWORK_ID, 121).await;
        harness.mock_data_source.expect_get_current_block_number().returning(|| Ok(123));
        // Return a block with two transactions.
        harness
            .mock_data_source
            .expect_fetch_block_core_data()
            .with(eq(122))
            .returning(move |block_num| Ok((block.clone().number(block_num).build(), vec![])));
        // The core assertion: fetch_receipts is called with both transaction hashes.
        harness
            .mock_data_source
            .expect_fetch_receipts()
            .with(eq(vec![tx_hash1, tx_hash2]))
            .returning(move |_| Ok(expected_receipts.clone()));

        let supervisor = harness.build();
        let (tx, _rx) = mpsc::channel(10);

        // Act
        let result = supervisor.monitor_cycle(tx).await;

        // Assert
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_monitor_cycle_waits_when_chain_is_shorter_than_confirmation_buffer() {
        // Arrange
        let mut harness = SupervisorTestHarness::new().await;
        harness.config.confirmation_blocks = 5;

        // Simulate the current chain head is at block 4, which is less than the
        // confirmation buffer.
        harness.mock_data_source.expect_get_current_block_number().returning(|| Ok(4));
        harness.mock_data_source.expect_fetch_block_core_data().times(0);

        let supervisor = harness.build();
        let (tx, _rx) = mpsc::channel(10);

        // Act
        let result = supervisor.monitor_cycle(tx).await;

        // Assert
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_monitor_cycle_waits_when_caught_up_to_confirmation_buffer() {
        // Arrange
        let mut harness = SupervisorTestHarness::new().await;
        harness.config.confirmation_blocks = 5;

        // Set the last processed block to 95.
        let _ = harness.state_repo.set_last_processed_block(TEST_NETWORK_ID, 95).await;
        // Current chain head is 100. The safe block is 100 - 5 = 95.
        harness.mock_data_source.expect_get_current_block_number().returning(|| Ok(100));
        // The next block to process would be 96, which is greater than the safe block.
        // The cycle should wait.
        harness.mock_data_source.expect_fetch_block_core_data().times(0);

        let supervisor = harness.build();
        let (tx, _rx) = mpsc::channel(10);

        // Act
        let result = supervisor.monitor_cycle(tx).await;

        // Assert
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_monitor_cycle_stops_when_channel_is_closed() {
        // Arrange
        let mut harness = SupervisorTestHarness::new().await;

        harness.mock_filtering_engine.expect_requires_receipt_data().returning(|| false);
        // Set the last processed block to 121.
        let _ = harness.state_repo.set_last_processed_block(TEST_NETWORK_ID, 121).await;
        harness.mock_data_source.expect_get_current_block_number().returning(|| Ok(123));
        harness
            .mock_data_source
            .expect_fetch_block_core_data()
            .with(eq(122))
            .returning(|block_num| Ok((BlockBuilder::new().number(block_num).build(), vec![])));

        let supervisor = harness.build();
        let (tx, rx) = mpsc::channel(10);

        // Drop the receiver to immediately close the channel.
        drop(rx);

        // Act
        let result = supervisor.monitor_cycle(tx).await;

        // Assert
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SupervisorError::ChannelClosed));
    }
}
