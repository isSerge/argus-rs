//! The Supervisor module manages the lifecycle of the Argus application, coordinating between the engine, data sources, and state repository.

use std::{sync::Arc, time::Duration};

use crate::{
    abi::AbiService,
    config::AppConfig,
    engine::{
        block_processor::BlockProcessor,
        filtering::{FilteringEngine, RhaiFilteringEngine},
    },
    models::{BlockData, DecodedBlockData},
    persistence::traits::StateRepository,
    providers::traits::DataSource,
};
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
}

/// The SupervisorBuilder is used to construct a Supervisor instance with all necessary components.
pub struct SupervisorBuilder {
    config: Option<AppConfig>,
    state: Option<Arc<dyn StateRepository>>,
    abi_service: Option<Arc<AbiService>>,
    data_source: Option<Box<dyn DataSource>>,
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
        unimplemented!()
    }

    /// Creates a new SupervisorBuilder to configure and build a Supervisor instance.
    pub fn builder() -> SupervisorBuilder {
        SupervisorBuilder::new()
    }
}

impl SupervisorBuilder {
    /// Creates a new SupervisorBuilder instance.
    pub fn new() -> Self {
        Self {
            config: None,
            state: None,
            abi_service: None,
            data_source: None,
        }
    }

    /// Sets the configuration for the Supervisor.
    pub fn config(mut self, config: AppConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Sets the state repository for the Supervisor.
    pub fn state(mut self, state: Arc<dyn StateRepository>) -> Self {
        self.state = Some(state);
        self
    }

    /// Sets the ABI service for the Supervisor.
    pub fn abi_service(mut self, abi_service: Arc<AbiService>) -> Self {
        self.abi_service = Some(abi_service);
        self
    }

    /// Sets the data source for the Supervisor.
    pub fn data_source(mut self, data_source: Box<dyn DataSource>) -> Self {
        self.data_source = Some(data_source);
        self
    }

    /// Builds the Supervisor instance, validating all required components are set.
    pub async fn build(self) -> Result<Supervisor, SupervisorError> {
        let config = self.config.ok_or(SupervisorError::MissingConfig)?;
        let state = self.state.ok_or(SupervisorError::MissingStateRepository)?;
        let abi_service = self.abi_service.ok_or(SupervisorError::MissingAbiService)?;
        let data_source = self.data_source.ok_or(SupervisorError::MissingDataSource)?;

        // Always load the monitors for the filtering engine from the database,
        // as it's the single source of truth for the running application.
        tracing::debug!(network_id = %config.network_id, "Loading monitors from database for filtering engine...");
        let monitors = state.get_monitors(&config.network_id).await?;
        tracing::info!(count = monitors.len(), network_id = %config.network_id, "Loaded monitors from database for filtering engine.");

        Ok(Supervisor::new(
            config.clone(), // TODO: remove later
            state,
            data_source,
            BlockProcessor::new(abi_service),
            Arc::new(RhaiFilteringEngine::new(monitors, config.rhai)),
        ))
    }
}
