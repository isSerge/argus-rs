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
    models::{BlockData, DecodedBlockData},
    persistence::{sqlite::SqliteStateRepository, traits::StateRepository},
    providers::{
        rpc::{EvmRpcSource, create_provider},
        traits::DataSource,
    },
};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::{
    signal,
    sync::{mpsc, watch},
    task::JoinHandle,
    time::{self, Duration},
};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

async fn main_loop(
    repo: &SqliteStateRepository,
    data_source: &impl DataSource,
    block_processor: &BlockProcessor,
    needs_receipts: bool,
    config: &AppConfig,
    shutdown_rx: watch::Receiver<bool>,
    decoded_blocks_tx: mpsc::Sender<DecodedBlockData>,
) {
    let mut shutdown_rx_clone = shutdown_rx.clone();
    loop {
        tokio::select! {
            _ = shutdown_rx_clone.changed() => {
                tracing::info!("Shutdown signal received in main loop, exiting.");
                return;
            }
            result = monitor_cycle(
                repo,
                data_source,
                block_processor,
                needs_receipts,
                &config.network_id,
                config.block_chunk_size,
                config.confirmation_blocks,
                shutdown_rx.clone(),
                decoded_blocks_tx.clone(),
            ) => {
                if let Err(e) = result {
                    tracing::error!(error = %e, "Error in monitoring cycle.");
                }
                time::sleep(Duration::from_millis(config.polling_interval_ms)).await;
            }
        }
    }
}

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
    let repo = Arc::new(SqliteStateRepository::new(&config.database_url).await?);
    repo.run_migrations().await?;
    tracing::info!("Database migrations completed.");

    // Load monitors from the configuration file if specified. This will overwrite
    // existing monitors for the same network in the database.
    if let Some(monitor_config_path) = &config.monitor_config_path
        && let Err(e) =
            load_monitors_from_file(repo.as_ref(), &config.network_id, monitor_config_path).await
        {
            tracing::error!(error = %e, "Failed to load monitors from file, continuing with monitors already in database (if any).");
        }

    // Always load the monitors for the filtering engine from the database,
    // as it's the single source of truth for the running application.
    tracing::debug!(network_id = %config.network_id, "Loading monitors from database for filtering engine...");
    let monitors_for_engine = repo.get_monitors(&config.network_id).await?;
    tracing::info!(count = monitors_for_engine.len(), network_id = %config.network_id, "Loaded monitors from database for filtering engine.");

    tracing::debug!(rpc_urls = ?config.rpc_urls, "Initializing EVM data source...");
    let provider = create_provider(config.rpc_urls.clone(), config.retry_config.clone())?;
    let evm_data_source = EvmRpcSource::new(provider);
    tracing::info!(retry_policy = ?config.retry_config, "EVM data source initialized with fallback and retry policy.");

    // Initialize BlockProcessor components
    tracing::debug!("Initializing ABI service and BlockProcessor...");
    let abi_service = Arc::new(AbiService::new());
    let filtering_engine = RhaiFilteringEngine::new(monitors_for_engine, config.rhai.clone());

    // Determine once if any monitor requires receipt data
    let needs_receipts = filtering_engine.requires_receipt_data();
    if needs_receipts {
        tracing::info!("Monitors require receipt data - will fetch receipts for all transactions.");
    } else {
        tracing::info!("No monitors require receipt data - will skip receipt fetching for better performance.");
    }

    let block_processor = BlockProcessor::new(abi_service);
    tracing::info!("BlockProcessor initialized successfully.");

    tracing::info!(network_id = %config.network_id, "Starting EVM monitor.");

    // Create a channel for decoded blocks
    let (decoded_blocks_tx, decoded_blocks_rx) =
        tokio::sync::mpsc::channel::<DecodedBlockData>(config.block_chunk_size as usize * 2);

    // Spawn the filtering engine task
    let filtering_engine_arc = Arc::new(filtering_engine);
    let filtering_engine_task = tokio::spawn(async move {
        filtering_engine_arc.run(decoded_blocks_rx).await;
    });

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let shutdown_task_handle = setup_shutdown_listener(shutdown_tx);

    main_loop(
        &repo,
        &evm_data_source,
        &block_processor,
        needs_receipts,
        &config,
        shutdown_rx,
        decoded_blocks_tx,
    )
    .await;

    tracing::info!("Main loop terminated. Initiating graceful shutdown.");
    let shutdown_timeout = Duration::from_secs(config.shutdown_timeout_secs);
    let shutdown_deadline = tokio::time::Instant::now() + shutdown_timeout;

    let shutdown_logic = async {
        tracing::info!("Waiting for filtering engine to shut down...");
        if let Err(e) = filtering_engine_task.await {
            tracing::warn!("Filtering engine task panicked or returned an error: {}", e);
        } else {
            tracing::info!("Filtering engine task completed successfully.");
        }
        graceful_cleanup(repo.as_ref(), &config.network_id).await
    };

    match tokio::time::timeout_at(shutdown_deadline, shutdown_logic).await {
        Ok(Ok(())) => tracing::info!("Graceful shutdown completed successfully."),
        Ok(Err(e)) => tracing::error!("Error during graceful cleanup: {}", e),
        Err(_) => tracing::warn!("Graceful shutdown timed out after {} seconds, forcing exit.", shutdown_timeout.as_secs()),
    }

    shutdown_task_handle.abort();
    tracing::info!("Application shutdown complete.");
    Ok(())
}

fn setup_shutdown_listener(shutdown_tx: watch::Sender<bool>) -> JoinHandle<()> {
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
            _ = ctrl_c => tracing::info!("SIGINT (Ctrl+C) received, initiating graceful shutdown."),
            _ = terminate => tracing::info!("SIGTERM received, initiating graceful shutdown."),
        }
        if let Err(e) = shutdown_tx.send(true) {
            tracing::warn!("Failed to send shutdown signal: {}", e);
        }
    })
}

#[tracing::instrument(
    skip(
        repo,
        data_source,
        block_processor,
        shutdown_rx,
        decoded_blocks_tx
    ),
    level = "debug"
)]
async fn monitor_cycle(
    repo: &SqliteStateRepository,
    data_source: &impl DataSource,
    block_processor: &BlockProcessor,
    needs_receipts: bool,
    network_id: &str,
    block_chunk_size: u64,
    confirmation_blocks: u64,
    shutdown_rx: watch::Receiver<bool>,
    decoded_blocks_tx: mpsc::Sender<DecodedBlockData>,
) -> Result<(), Box<dyn std::error::Error>> {
    let last_processed_block = repo.get_last_processed_block(network_id).await?;
    let current_block = data_source.get_current_block_number().await?;

    if current_block < confirmation_blocks {
        tracing::info!("Chain is shorter than the confirmation buffer. Waiting for more blocks.");
        return Ok(());
    }

    let from_block = last_processed_block.map_or_else(
        || current_block.saturating_sub(100),
        |block| block + 1,
    );
    let safe_to_block = current_block.saturating_sub(confirmation_blocks);

    if from_block > safe_to_block {
        tracing::info!("Caught up to confirmation buffer. Waiting for more blocks.");
        return Ok(());
    }

    let to_block = std::cmp::min(from_block + block_chunk_size, safe_to_block);
    tracing::info!(from_block = from_block, to_block = to_block, "Processing block range.");

    let mut last_processed = last_processed_block;
    let mut blocks_to_process = Vec::new();
    for block_num in from_block..=to_block {
        if *shutdown_rx.borrow() {
            tracing::info!("Shutdown signal detected during block collection, stopping.");
            break;
        }
        let (block, logs) = data_source.fetch_block_core_data(block_num).await?;
        let receipts = fetch_receipts_if_needed(data_source, &block, needs_receipts, network_id, block_num).await;
        blocks_to_process.push(BlockData::from_raw_data(block, receipts, logs));
    }

    if !blocks_to_process.is_empty() {
        match block_processor.process_blocks_batch(blocks_to_process).await {
            Ok(decoded_blocks) => {
                for decoded_block in decoded_blocks {
                    let block_num = decoded_block.block_number;
                    if decoded_blocks_tx.send(decoded_block).await.is_err() {
                        tracing::warn!("Decoded blocks channel closed, stopping further processing.");
                        return Err("Filtering engine receiver dropped".into());
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
        && Some(valid_last_processed) > last_processed_block {
            repo.set_last_processed_block(network_id, valid_last_processed).await?;
            tracing::info!(last_processed_block = valid_last_processed, "Last processed block updated successfully.");
        }

    Ok(())
}

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
            tracing::warn!(network_id = %network_id, block_number = block_num, error = %e, "Failed to fetch receipts, proceeding without them.");
            HashMap::new()
        }
    }
}

async fn graceful_cleanup(
    repo: &SqliteStateRepository,
    network_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Starting graceful cleanup...");
    if let Err(e) = repo.flush().await {
        tracing::error!(error = %e, "Failed to flush pending writes, but continuing cleanup.");
    }
    if let Err(e) = repo.cleanup().await {
        tracing::error!(error = %e, "Failed to perform repository cleanup, but continuing.");
    }
    match repo.get_last_processed_block(network_id).await {
        Ok(Some(last_block)) => tracing::info!(last_processed_block = last_block, "Final state: last processed block recorded."),
        Ok(None) => tracing::info!("Final state: no blocks have been processed yet."),
        Err(e) => tracing::warn!(error = %e, "Could not retrieve final state during cleanup."),
    }
    repo.close().await;
    tracing::info!("Database connections closed.");
    Ok(())
}

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
    repo.clear_monitors(network_id).await?;
    repo.add_monitors(network_id, monitors).await?;
    tracing::info!(count = count, network_id = %network_id, "Monitors from file stored in database.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::{primitives::{B256, TxHash}, rpc::types::{Block, Log, TransactionReceipt}};
    use argus::{providers::traits::DataSourceError, test_helpers::{BlockBuilder, ReceiptBuilder, TransactionBuilder}};
    use mockall::predicate::*;
    use std::collections::HashMap;

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
        mock_data_source.expect_fetch_receipts().times(0);
        let block = BlockBuilder::new().build();
        let result = fetch_receipts_if_needed(&mock_data_source, &block, false, "test_network", 123).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_receipts_if_needed_empty_block() {
        let mut mock_data_source = MockTestDataSource::new();
        mock_data_source.expect_fetch_receipts().times(0);
        let block = BlockBuilder::new().build();
        let result = fetch_receipts_if_needed(&mock_data_source, &block, true, "test_network", 123).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_receipts_if_needed_success() {
        let mut mock_data_source = MockTestDataSource::new();
        let tx_hash = B256::from([1u8; 32]);
        let receipt = ReceiptBuilder::new().transaction_hash(tx_hash).block_number(123).build();
        let mut expected_receipts = HashMap::new();
        expected_receipts.insert(tx_hash, receipt.clone());
        mock_data_source.expect_fetch_receipts().with(eq(vec![tx_hash])).times(1).returning(move |_| Ok(expected_receipts.clone()));
        let tx = TransactionBuilder::new().hash(tx_hash).build();
        let block = BlockBuilder::new().transaction(tx).build();
        let result = fetch_receipts_if_needed(&mock_data_source, &block, true, "test_network", 123).await;
        assert_eq!(result.len(), 1);
        assert!(result.contains_key(&tx_hash));
    }

    #[tokio::test]
    async fn test_fetch_receipts_if_needed_failure_handling() {
        let mut mock_data_source = MockTestDataSource::new();
        let tx_hash = B256::from([1u8; 32]);
        mock_data_source.expect_fetch_receipts().with(eq(vec![tx_hash])).times(1).returning(|_| Err(DataSourceError::Provider(Box::new(std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "Connection failed")))));
        let tx = TransactionBuilder::new().hash(tx_hash).build();
        let block = BlockBuilder::new().transaction(tx).build();
        let result = fetch_receipts_if_needed(&mock_data_source, &block, true, "test_network", 123).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_receipts_if_needed_multiple_transactions() {
        let mut mock_data_source = MockTestDataSource::new();
        let tx_hash1 = B256::from([1u8; 32]);
        let tx_hash2 = B256::from([2u8; 32]);
        let receipt1 = ReceiptBuilder::new().transaction_hash(tx_hash1).block_number(123).build();
        let receipt2 = ReceiptBuilder::new().transaction_hash(tx_hash2).block_number(123).build();
        let mut expected_receipts = HashMap::new();
        expected_receipts.insert(tx_hash1, receipt1);
        expected_receipts.insert(tx_hash2, receipt2);
        mock_data_source.expect_fetch_receipts().with(always()).times(1).returning(move |_| Ok(expected_receipts.clone()));
        let tx1 = TransactionBuilder::new().hash(tx_hash1).build();
        let tx2 = TransactionBuilder::new().hash(tx_hash2).build();
        let block = BlockBuilder::new().transaction(tx1).transaction(tx2).build();
        let result = fetch_receipts_if_needed(&mock_data_source, &block, true, "test_network", 123).await;
        assert_eq!(result.len(), 2);
        assert!(result.contains_key(&tx_hash1));
        assert!(result.contains_key(&tx_hash2));
    }
}
