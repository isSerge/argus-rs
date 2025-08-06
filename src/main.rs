use alloy::{
    primitives::TxHash,
    rpc::types::{Block, TransactionReceipt},
};
use argus::{
    abi::AbiService,
    config::{AppConfig, MonitorLoader},
    engine::{
        block_processor::{BlockProcessor, BlockProcessorError},
        filtering::{FilteringEngine, RhaiFilteringEngine},
    },
    models::BlockData,
    persistence::{sqlite::SqliteStateRepository, traits::StateRepository},
    providers::{
        rpc::{EvmRpcSource, create_provider},
        traits::DataSource,
    },
};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::{
    signal,
    time::{self, Duration},
};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

#[tokio::main]
#[tracing::instrument(level = "info")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing subscriber
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    tracing::debug!("Loading application configuration...");
    let config = AppConfig::new()?;
    tracing::debug!(database_url = %config.database_url, rpc_urls = ?config.rpc_urls, network_id = %config.network_id, "Configuration loaded.");

    tracing::debug!("Initializing state repository...");
    let repo = SqliteStateRepository::new(&config.database_url).await?;
    tracing::debug!("Running database migrations...");
    repo.run_migrations().await?;
    tracing::info!("Database migrations completed.");

    // Load monitors from the configuration file if specified. This will overwrite
    // existing monitors for the same network in the database.
    if let Some(monitor_config_path) = &config.monitor_config_path {
        if let Err(e) = load_monitors_from_file(&repo, &config.network_id, monitor_config_path).await {
            tracing::error!(error = %e, "Failed to load monitors from file, continuing with monitors already in database (if any).");
        }
    }

    // Always load the monitors for the filtering engine from the database,
    // as it's the single source of truth for the running application.
    tracing::debug!(network_id = %config.network_id, "Loading monitors from database for filtering engine...");
    let monitors_for_engine = repo.get_monitors(&config.network_id).await?;
    tracing::info!(count = monitors_for_engine.len(), network_id = %config.network_id, "Loaded monitors from database for filtering engine.");

    tracing::debug!(rpc_urls = ?config.rpc_urls, "Initializing EVM data source...");
    let provider = create_provider(config.rpc_urls, config.retry_config.clone())?;
    let evm_data_source = EvmRpcSource::new(provider);
    tracing::info!(retry_policy = ?config.retry_config, "EVM data source initialized with fallback and retry policy.");

    // Initialize BlockProcessor components
    tracing::debug!("Initializing ABI service and BlockProcessor...");
    let abi_service = Arc::new(AbiService::new());
    let filtering_engine = RhaiFilteringEngine::new(monitors_for_engine, config.rhai);

    // Determine once if any monitor requires receipt data
    let needs_receipts = filtering_engine.requires_receipt_data();
    if needs_receipts {
        tracing::info!("Monitors require receipt data - will fetch receipts for all transactions.");
    } else {
        tracing::info!(
            "No monitors require receipt data - will skip receipt fetching for better performance."
        );
    }

    let block_processor = BlockProcessor::new(abi_service);
    tracing::info!("BlockProcessor initialized successfully.");

    tracing::info!(network_id = %config.network_id, "Starting EVM monitor.");

    // Create a watch channel for shutdown signals
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    // Spawn a task to listen for multiple shutdown signals
    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
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
            _ = ctrl_c => {
                tracing::info!("SIGINT (Ctrl+C) received, initiating graceful shutdown.");
            }
            _ = terminate => {
                tracing::info!("SIGTERM received, initiating graceful shutdown.");
            }
        }

        if let Err(e) = shutdown_tx_clone.send(true) {
            tracing::warn!("Failed to send shutdown signal: {}", e);
            // Return from the task to allow proper cleanup
        }
    });

    // Main monitoring loop with shutdown timeout
    let mut shutdown_rx_loop = shutdown_rx.clone();
    let shutdown_timeout = Duration::from_secs(config.shutdown_timeout_secs);

    loop {
        tokio::select! {
            _ = shutdown_rx_loop.changed() => {
                tracing::info!("Shutdown signal received, initiating graceful shutdown.");

                // Give ongoing operations time to complete gracefully
                tracing::info!("Waiting up to {} seconds for ongoing operations to complete...", shutdown_timeout.as_secs());

                // Set a deadline for shutdown completion
                let shutdown_deadline = tokio::time::Instant::now() + shutdown_timeout;

                // Attempt graceful cleanup with timeout
                let cleanup_result = tokio::time::timeout_at(
                    shutdown_deadline,
                    graceful_cleanup(&repo, &config.network_id)
                ).await;

                match cleanup_result {
                    Ok(Ok(())) => {
                        tracing::info!("Graceful cleanup completed successfully.");
                    }
                    Ok(Err(e)) => {
                        tracing::error!("Error during graceful cleanup: {}", e);
                    }
                    Err(_) => {
                        tracing::warn!("Graceful cleanup timed out after {} seconds, forcing shutdown.", shutdown_timeout.as_secs());
                    }
                }

                break; // Exit the loop
            }
            result = monitor_cycle(
                &repo,
                &evm_data_source,
                &block_processor,
                needs_receipts,
                &config.network_id,
                config.block_chunk_size,
                config.confirmation_blocks,
                shutdown_rx.clone(),
            ) => {
                match result {
                    Ok(_) => {
                        tracing::trace!("Monitoring cycle completed successfully.");
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Error in monitoring cycle.");
                        // Continue monitoring even if there's an error
                    }
                }
                // Wait for the configured polling interval before the next cycle
                time::sleep(Duration::from_millis(config.polling_interval_ms)).await;
            }
        }
    }

    tracing::info!("Application shutdown complete.");
    Ok(())
}

#[tracing::instrument(skip(repo, data_source, block_processor, shutdown_rx), level = "debug")]
async fn monitor_cycle(
    repo: &SqliteStateRepository,
    data_source: &impl DataSource,
    block_processor: &BlockProcessor,
    needs_receipts: bool,
    network_id: &str,
    block_chunk_size: u64,
    confirmation_blocks: u64,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Read the last processed block from the StateRepository
    tracing::debug!(network_id = %network_id, "Fetching last processed block from state repository.");
    let last_processed_block = repo.get_last_processed_block(network_id).await?;
    tracing::debug!(network_id = %network_id, last_processed_block = ?last_processed_block, "Last processed block retrieved.");

    // 2. Determine a target "to block"
    tracing::debug!(network_id = %network_id, "Fetching current block number from data source.");
    let current_block = data_source.get_current_block_number().await?;
    tracing::debug!(network_id = %network_id, current_block = %current_block, "Current block number retrieved.");

    // Check if the chain is long enough to handle the confirmation buffer
    if current_block < confirmation_blocks {
        tracing::info!(
            network_id = %network_id,
            current_block = %current_block,
            confirmation_blocks = %confirmation_blocks,
            "Chain is shorter than the confirmation buffer. Waiting for more blocks."
        );
        return Ok(());
    }

    let from_block = match last_processed_block {
        Some(block) => {
            tracing::debug!(network_id = %network_id, last_processed_block = %block, "Starting processing from the next block.");
            block + 1
        }
        None => {
            // If no blocks have been processed, start from a recent block (e.g., current - 100)
            // to avoid processing the entire blockchain history on first run
            let start_block = current_block.saturating_sub(100);
            tracing::info!(network_id = %network_id, current_block = %current_block, start_block = %start_block, "No last processed block found, starting from a recent block to avoid full history scan.");
            start_block
        }
    };

    let safe_to_block = current_block.saturating_sub(confirmation_blocks);

    // Don't process if we're already caught up to the safe block
    if from_block > safe_to_block {
        tracing::info!(
            network_id = %network_id,
            current_block = %current_block,
            from_block = %from_block,
            "Caught up to confirmation buffer. Waiting for more blocks."
        );
        return Ok(());
    }

    // Process in smaller chunks to avoid hitting RPC limits
    let to_block = std::cmp::min(from_block + block_chunk_size, safe_to_block);

    tracing::info!(
        network_id = %network_id,
        from_block = %from_block,
        to_block = %to_block,
        "Processing block range."
    );

    // 3. Process blocks in batch with shutdown checks
    let mut last_processed = last_processed_block; // Use the actual last processed block from DB
    let mut blocks_processed_this_cycle = 0;

    // Collect blocks to process in batch
    let mut blocks_to_process = Vec::new();

    for block_num in from_block..=to_block {
        // Check for shutdown signal before processing each block
        if *shutdown_rx.borrow() {
            tracing::info!(
                network_id = %network_id,
                block_number = block_num,
                blocks_processed_this_cycle = blocks_processed_this_cycle,
                blocks_pending = blocks_to_process.len(),
                "Shutdown signal detected, processing pending blocks then stopping."
            );
            break;
        }

        // Fetch block data
        let (block, logs) = data_source.fetch_block_core_data(block_num).await?;

        // Fetch receipts if needed
        let receipts =
            fetch_receipts_if_needed(data_source, &block, needs_receipts, network_id, block_num)
                .await;

        let block_data = BlockData::from_raw_data(block, receipts, logs);
        blocks_to_process.push((block_num, block_data));
    }

    // Process all collected blocks in batch
    if !blocks_to_process.is_empty() {
        tracing::debug!(
            network_id = %network_id,
            block_count = blocks_to_process.len(),
            "Processing blocks in batch."
        );

        let block_data_vec: Vec<BlockData> = blocks_to_process
            .iter()
            .map(|(_, data)| data.clone())
            .collect();

        match block_processor.process_blocks_batch(block_data_vec).await {
            Ok(decoded_blocks) => {
                // Log results for each block
                for (block_num, block_data) in &blocks_to_process {
                    let tx_count = block_data.block.transactions.len();
                    let log_count = block_data.logs.values().map(Vec::len).sum::<usize>();
                    let block_hash = block_data.block.header.hash;

                    tracing::info!(
                        network_id = %network_id,
                        block_number = %block_num,
                        block_hash = %block_hash,
                        tx_count = tx_count,
                        log_count = log_count,
                        "Processed block in batch."
                    );

                    last_processed = Some(*block_num);
                    blocks_processed_this_cycle += 1;
                }

                // Log overall batch results
                tracing::info!(
                    network_id = %network_id,
                    blocks_processed = blocks_processed_this_cycle,
                    total_matches = decoded_blocks.len(),
                    "Batch processing completed successfully."
                );

                // TODO: send decoded_blocks to the filtering engine
                if !decoded_blocks.is_empty() {
                    tracing::info!(
                        network_id = %network_id,
                        matches_count = decoded_blocks.len(),
                        "Found monitor matches in batch."
                    );
                }
            }
            Err(BlockProcessorError::AbiService(e)) => {
                tracing::warn!(
                    network_id = %network_id,
                    error = %e,
                    "ABI service error during batch processing, continuing."
                );
                // Still update the processed blocks since data was fetched successfully
                for (block_num, _) in &blocks_to_process {
                    last_processed = Some(*block_num);
                    blocks_processed_this_cycle += 1;
                }
            }
        }
    }

    // Handle shutdown state saving after batch processing
    if *shutdown_rx.borrow() {
        if let Some(valid_last_processed) = last_processed {
            let emergency_message = if blocks_processed_this_cycle == 0 {
                "Shutdown during cycle, no blocks processed this cycle".to_string()
            } else {
                format!(
                    "Shutdown during cycle, processed {blocks_processed_this_cycle} blocks this cycle"
                )
            };
            if let Err(e) = repo
                .save_emergency_state(network_id, valid_last_processed, &emergency_message)
                .await
            {
                tracing::error!(error = %e, "Failed to save emergency state during shutdown.");
            }
        } else {
            tracing::info!(
                network_id = %network_id,
                "Shutdown during initial processing - no valid last processed block to save as emergency state."
            );
        }
    }

    // 4. Update the StateRepository with the new last processed block number
    // Only update if we actually processed any blocks this cycle
    if let Some(valid_last_processed) = last_processed {
        tracing::debug!(network_id = %network_id, new_last_processed_block = %valid_last_processed, "Updating last processed block in state repository.");
        repo.set_last_processed_block(network_id, valid_last_processed)
            .await?;
        tracing::info!(network_id = %network_id, last_processed_block = %valid_last_processed, "Last processed block updated successfully.");
    } else {
        tracing::debug!(network_id = %network_id, "No blocks processed this cycle, skipping state update.");
    }

    Ok(())
}

/// Helper function to fetch receipts if needed
async fn fetch_receipts_if_needed(
    data_source: &impl DataSource,
    block: &Block,
    needs_receipts: bool,
    network_id: &str,
    block_num: u64,
) -> HashMap<TxHash, TransactionReceipt> {
    if !needs_receipts {
        return HashMap::new();
    }

    let tx_hashes: Vec<_> = block.transactions.hashes().collect();
    if tx_hashes.is_empty() {
        return HashMap::new();
    }

    match data_source.fetch_receipts(&tx_hashes).await {
        Ok(receipts) => receipts,
        Err(e) => {
            tracing::warn!(
                network_id = %network_id,
                block_number = block_num,
                error = %e,
                "Failed to fetch receipts, proceeding without them."
            );
            HashMap::new()
        }
    }
}

/// Performs graceful cleanup operations during shutdown
async fn graceful_cleanup(
    repo: &SqliteStateRepository,
    network_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Starting graceful cleanup...");

    // 1. Flush any pending state updates to ensure data integrity
    tracing::debug!("Flushing pending database writes...");
    if let Err(e) = repo.flush().await {
        tracing::error!(error = %e, "Failed to flush pending writes, but continuing cleanup.");
        // Don't return early - continue with other cleanup operations
    } else {
        tracing::debug!("Database writes flushed successfully.");
    }

    // 2. Perform repository-specific cleanup (WAL checkpoint, etc.)
    tracing::debug!("Performing repository cleanup...");
    if let Err(e) = repo.cleanup().await {
        tracing::error!(error = %e, "Failed to perform repository cleanup, but continuing.");
    } else {
        tracing::debug!("Repository cleanup completed successfully.");
    }

    // 3. Log final state for debugging
    tracing::debug!("Retrieving final processed block state...");
    match repo.get_last_processed_block(network_id).await {
        Ok(Some(last_block)) => {
            tracing::info!(
                network_id = %network_id,
                last_processed_block = %last_block,
                "Final state: last processed block recorded."
            );
        }
        Ok(None) => {
            tracing::info!(
                network_id = %network_id,
                "Final state: no blocks have been processed yet."
            );
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                network_id = %network_id,
                "Could not retrieve final state during cleanup."
            );
        }
    }

    // 4. Close database connections gracefully
    tracing::debug!("Closing database connections...");
    repo.close().await;
    tracing::debug!("Database connections closed.");

    // 5. Allow some time for any background tasks to complete
    tracing::debug!("Allowing time for background tasks to complete...");
    tokio::time::sleep(Duration::from_millis(100)).await;

    Ok(())
}

/// Loads monitors from a specified configuration file, clears existing monitors
/// for the network, and stores the new ones in the state repository.
async fn load_monitors_from_file(
    repo: &impl StateRepository,
    network_id: &str,
    config_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::debug!(config_path = %config_path, "Loading monitors from configuration file...");
    
    let monitor_loader = MonitorLoader::new(PathBuf::from(config_path));
    let monitors = monitor_loader.load()?;
    let count = monitors.len();

    tracing::info!(count = count, "Loaded monitors from configuration file.");

    // Atomically clear and add new monitors
    repo.clear_monitors(network_id).await?;
    repo.add_monitors(network_id, monitors).await?;

    tracing::info!(count = count, network_id = %network_id, "Monitors from file stored in database.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::{
        primitives::{B256, TxHash},
        rpc::types::{Block, Log, TransactionReceipt},
    };
    use argus::{
        providers::traits::DataSourceError,
        test_helpers::{BlockBuilder, ReceiptBuilder, TransactionBuilder},
    };
    use mockall::predicate::*;
    use std::collections::HashMap;

    // Mock DataSource for testing
    mockall::mock! {
        TestDataSource {}

        #[async_trait::async_trait]
        impl DataSource for TestDataSource {
            async fn fetch_block_core_data(&self, block_number: u64) -> Result<(Block, Vec<Log>), DataSourceError>;
            async fn get_current_block_number(&self) -> Result<u64, DataSourceError>;
            async fn fetch_receipts(&self, tx_hashes: &[TxHash]) -> Result<HashMap<TxHash, TransactionReceipt>, DataSourceError>;
        }
    }

    #[tokio::test]
    async fn test_fetch_receipts_if_needed_when_not_needed() {
        let mut mock_data_source = MockTestDataSource::new();

        // Should not call fetch_receipts when needs_receipts is false
        mock_data_source.expect_fetch_receipts().times(0);

        let block = BlockBuilder::new().build();
        let result = fetch_receipts_if_needed(
            &mock_data_source,
            &block,
            false, // needs_receipts = false
            "test_network",
            123,
        )
        .await;

        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_receipts_if_needed_empty_block() {
        let mut mock_data_source = MockTestDataSource::new();

        // Should not call fetch_receipts when block has no transactions
        mock_data_source.expect_fetch_receipts().times(0);

        let block = BlockBuilder::new().build(); // Empty block with no transactions
        let result = fetch_receipts_if_needed(
            &mock_data_source,
            &block,
            true, // needs_receipts = true
            "test_network",
            123,
        )
        .await;

        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_receipts_if_needed_success() {
        let mut mock_data_source = MockTestDataSource::new();

        let tx_hash = B256::from([1u8; 32]);
        let receipt = ReceiptBuilder::new()
            .transaction_hash(tx_hash)
            .block_number(123)
            .build();

        let mut expected_receipts = HashMap::new();
        expected_receipts.insert(tx_hash, receipt.clone());

        mock_data_source
            .expect_fetch_receipts()
            .with(eq(vec![tx_hash]))
            .times(1)
            .returning(move |_| Ok(expected_receipts.clone()));

        let tx = TransactionBuilder::new().hash(tx_hash).build();
        let block = BlockBuilder::new().transaction(tx).build();

        let result = fetch_receipts_if_needed(
            &mock_data_source,
            &block,
            true, // needs_receipts = true
            "test_network",
            123,
        )
        .await;

        assert_eq!(result.len(), 1);
        assert!(result.contains_key(&tx_hash));
    }

    #[tokio::test]
    async fn test_fetch_receipts_if_needed_failure_handling() {
        let mut mock_data_source = MockTestDataSource::new();

        let tx_hash = B256::from([1u8; 32]);

        mock_data_source
            .expect_fetch_receipts()
            .with(eq(vec![tx_hash]))
            .times(1)
            .returning(|_| {
                Err(DataSourceError::Provider(Box::new(std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    "Connection failed",
                ))))
            });

        let tx = TransactionBuilder::new().hash(tx_hash).build();
        let block = BlockBuilder::new().transaction(tx).build();

        let result = fetch_receipts_if_needed(
            &mock_data_source,
            &block,
            true, // needs_receipts = true
            "test_network",
            123,
        )
        .await;

        // Should return empty HashMap on error, not panic
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_receipts_if_needed_multiple_transactions() {
        let mut mock_data_source = MockTestDataSource::new();

        let tx_hash1 = B256::from([1u8; 32]);
        let tx_hash2 = B256::from([2u8; 32]);

        let receipt1 = ReceiptBuilder::new()
            .transaction_hash(tx_hash1)
            .block_number(123)
            .build();
        let receipt2 = ReceiptBuilder::new()
            .transaction_hash(tx_hash2)
            .block_number(123)
            .build();

        let mut expected_receipts = HashMap::new();
        expected_receipts.insert(tx_hash1, receipt1);
        expected_receipts.insert(tx_hash2, receipt2);

        mock_data_source
            .expect_fetch_receipts()
            .with(always()) // Simplified predicate
            .times(1)
            .returning(move |_| Ok(expected_receipts.clone()));

        let tx1 = TransactionBuilder::new().hash(tx_hash1).build();
        let tx2 = TransactionBuilder::new().hash(tx_hash2).build();
        let block = BlockBuilder::new()
            .transaction(tx1)
            .transaction(tx2)
            .build();

        let result = fetch_receipts_if_needed(
            &mock_data_source,
            &block,
            true, // needs_receipts = true
            "test_network",
            123,
        )
        .await;

        assert_eq!(result.len(), 2);
        assert!(result.contains_key(&tx_hash1));
        assert!(result.contains_key(&tx_hash2));
    }
}
