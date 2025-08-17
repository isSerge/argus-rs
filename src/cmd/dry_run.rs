//! This module provides functionality to execute a dry run of the monitoring process
//! over a specified block range.

use std::sync::Arc;

use clap::Parser;
use thiserror::Error;

use crate::{
    abi::AbiService,
    config::{AppConfig, TriggerLoader, TriggerLoaderError},
    engine::{
        block_processor::{BlockProcessor, BlockProcessorError},
        filtering::{FilteringEngine, RhaiError, RhaiFilteringEngine},
        rhai::RhaiCompiler,
    },
    http_client::HttpClientPool,
    models::{monitor_match::MonitorMatch, BlockData},
    monitor::{MonitorLoader, MonitorLoaderError, MonitorValidationError, MonitorValidator},
    notification::NotificationService,
    providers::{
        rpc::{create_provider, EvmRpcSource, ProviderError},
        traits::{DataSource, DataSourceError},
    },
};

/// Errors that can occur during the execution of a dry run.
#[derive(Error, Debug)]
pub enum DryRunError {
    /// An error occurred while loading the application configuration.
    #[error("Config error: {0}")]
    Config(#[from] config::ConfigError),

    /// An error occurred while loading monitor definitions.
    #[error("Monitor loading error: {0}")]
    MonitorLoading(#[from] MonitorLoaderError),

    /// An error occurred while loading trigger definitions.
    #[error("Trigger loading error: {0}")]
    TriggerLoading(#[from] TriggerLoaderError),

    /// A monitor failed validation against the defined rules.
    #[error("Monitor validation error: {0}")]
    MonitorValidation(#[from] MonitorValidationError),

    /// An error occurred with the blockchain provider.
    #[error("Provider error: {0}")]
    Provider(#[from] ProviderError),

    /// An error occurred while fetching data from the blockchain.
    #[error("Data source error: {0}")]
    DataSource(#[from] DataSourceError),

    /// An error occurred during the block processing stage.
    #[error("Block processor error: {0}")]
    BlockProcessor(#[from] BlockProcessorError),

    /// An error occurred within the filtering engine, likely during script
    /// execution.
    #[error("Filtering engine error: {0}")]
    Filtering(#[from] RhaiError),

    /// An error occurred while serializing the final report to JSON.
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

/// A command to perform a dry run of monitors over a specified block range.
///
/// This command initializes the application's services in a one-shot mode to
/// test monitor configurations against historical blockchain data. It fetches,
/// processes, and filters data for each block in the range, dispatches real
/// notifications for any matches, and prints a JSON report of all matches to
/// standard output.
#[derive(Parser, Debug)]
pub struct DryRunArgs {
    /// Path to the monitor configuration file. If not provided, uses the path
    /// from the main `config.yaml`.
    #[arg(short, long)]
    monitor: Option<String>,
    /// The starting block number for the dry run (inclusive).
    #[arg(long)]
    from: u64,
    /// The ending block number for the dry run (inclusive).
    #[arg(long)]
    to: u64,
    /// Path to the triggers configuration file. If not provided, uses the path
    /// from the main `config.yaml`.
    #[arg(short, long)]
    triggers: Option<String>,
}

/// The main entry point for the `dry-run` command.
///
/// This function orchestrates the entire dry run process:
/// 1.  Loads the main application configuration.
/// 2.  Initializes all necessary services (data source, block processor,
///     filtering engine, etc.).
/// 3.  Loads and validates the monitor and trigger configurations.
/// 4.  Calls `run_dry_run_loop` to execute the core processing logic.
/// 5.  Serializes the results to a pretty JSON string and prints to stdout.
pub async fn execute(args: DryRunArgs) -> Result<(), DryRunError> {
    let config = AppConfig::new(None)?;

    // Init EVM data source for fetching blockchain data.
    let provider = create_provider(config.rpc_urls.clone(), config.rpc_retry_config.clone())?;
    let evm_source = EvmRpcSource::new(provider);

    // Init services for processing and decoding data.
    let abi_service = Arc::new(AbiService::new());
    let block_processor = BlockProcessor::new(Arc::clone(&abi_service));

    // Load and validate monitor and trigger configurations from files.
    let monitors_path = args.monitor.as_deref().unwrap_or(&config.monitor_config_path);
    let monitor_loader = MonitorLoader::new(monitors_path.into());
    let monitors = monitor_loader.load()?;
    let triggers_path = args.triggers.as_deref().unwrap_or(&config.trigger_config_path);
    let trigger_loader = TriggerLoader::new(triggers_path.into());
    let triggers = trigger_loader.load()?;

    let monitor_validator = MonitorValidator::new(&config.network_id);
    for monitor in monitors.iter() {
        tracing::debug!(monitor = %monitor.name, "Validating monitor...");
        monitor_validator.validate(&monitor)?;
    }
    tracing::info!("Monitor validation successful.");

    // Init services for notifications and filtering logic.
    let client_pool = Arc::new(HttpClientPool::new());
    let notification_service = NotificationService::new(triggers, client_pool);
    let rhai_compiler = Arc::new(RhaiCompiler::new(config.rhai.clone()));
    let filtering_engine = RhaiFilteringEngine::new(monitors, rhai_compiler, config.rhai.clone());

    // Execute the core processing loop.
    let matches = run_dry_run_loop(
        args.from,
        args.to,
        Box::new(evm_source),
        block_processor,
        filtering_engine,
        notification_service,
    )
    .await?;

    // Serialize and print the final report.
    let report = serde_json::to_string_pretty(&matches)?;
    println!("{}", report);

    Ok(())
}

/// The core processing logic for the dry run.
///
/// This function is decoupled from the concrete `EvmRpcSource` to facilitate
/// testing with a mock data source. It iterates through the specified block
/// range, fetches the necessary data, processes it, evaluates it against the
/// filtering engine, and dispatches notifications for any matches.
///
/// # Arguments
///
/// * `from_block` - The starting block number.
/// * `to_block` - The ending block number.
/// * `data_source` - A boxed trait object for fetching blockchain data.
/// * `block_processor` - The service for decoding raw block data.
/// * `filtering_engine` - The service for evaluating data against monitor
///   scripts.
/// * `notification_service` - The service for sending notifications.
///
/// # Returns
///
/// A `Result` containing a vector of all `MonitorMatch`es found during the run,
/// or a `DryRunError`.
async fn run_dry_run_loop(
    from_block: u64,
    to_block: u64,
    data_source: Box<dyn DataSource>,
    block_processor: BlockProcessor,
    filtering_engine: RhaiFilteringEngine,
    notification_service: NotificationService,
) -> Result<Vec<MonitorMatch>, DryRunError> {
    let mut matches: Vec<MonitorMatch> = Vec::new();
    let mut current_block = from_block;

    tracing::info!(from = from_block, to = to_block, "Starting block processing...");

    while current_block <= to_block {
        let (block, logs) = data_source.fetch_block_core_data(current_block).await?;

        // Conditionally fetch transaction receipts only if a monitor requires them.
        let receipts = if filtering_engine.requires_receipt_data() {
            let tx_hashes: Vec<_> = block.transactions.hashes().collect();
            if tx_hashes.is_empty() {
                Default::default()
            } else {
                data_source.fetch_receipts(&tx_hashes).await?
            }
        } else {
            Default::default()
        };

        // Process the raw block data into a decoded format.
        let block_data = BlockData::from_raw_data(block, receipts, logs);
        let decoded_blocks = block_processor.process_blocks_batch(vec![block_data]).await?;

        // Evaluate each item in the decoded block against the filtering engine.
        for decoded_block in decoded_blocks {
            for item in decoded_block.items {
                let item_matches = filtering_engine.evaluate_item(&item).await?;
                for m in item_matches {
                    // For every match, dispatch a notification and collect the result.
                    let _ = notification_service.execute(&m).await;
                    matches.push(m.clone());
                }
            }
        }
        current_block += 1;
    }
    tracing::info!("Block processing finished.");

    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        abi::AbiService,
        config::RhaiConfig,
        engine::{
            block_processor::BlockProcessor,
            filtering::RhaiFilteringEngine,
            rhai::RhaiCompiler,
        },
        http_client::HttpClientPool,
        models::monitor::Monitor,
        notification::NotificationService,
        providers::traits::MockDataSource,
        test_helpers::{BlockBuilder, TransactionBuilder},
    };
    use alloy::primitives::U256;
    use mockall::predicate::eq;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_run_dry_run_loop_succeeds() {
        // Arrange
        let from_block = 100;
        let to_block = 100;
        let monitor_script = "tx.value > bigint(\"10000000000000000000\")";

        // Create a mock data source
        let mut mock_data_source = MockDataSource::new();
        mock_data_source
            .expect_fetch_block_core_data()
            .with(eq(from_block))
            .times(1)
            .returning(|block_num| {
                let tx = TransactionBuilder::new()
                    .value(U256::MAX)
                    .block_number(block_num)
                    .build();
                let block = BlockBuilder::new().number(block_num).transaction(tx).build();
                Ok((block, vec![]))
            });

        // Create a monitor that will match the transaction
        let monitor = Monitor::from_config(
            "Test Monitor".to_string(),
            "testnet".to_string(),
            None,
            None,
            monitor_script.to_string(),
        );

        // Initialize other services
        let abi_service = Arc::new(AbiService::new());
        let block_processor = BlockProcessor::new(abi_service);
        let rhai_config = RhaiConfig::default();
        let rhai_compiler = Arc::new(RhaiCompiler::new(rhai_config.clone()));
        let filtering_engine =
            RhaiFilteringEngine::new(vec![monitor], rhai_compiler, rhai_config);
        let client_pool = Arc::new(HttpClientPool::new());
        let notification_service = NotificationService::new(vec![], client_pool);

        // Act
        let result = run_dry_run_loop(
            from_block,
            to_block,
            Box::new(mock_data_source),
            block_processor,
            filtering_engine,
            notification_service,
        )
        .await;

        // Assert
        assert!(result.is_ok());
        let matches = result.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].block_number, from_block);
    }

    #[tokio::test]
    async fn test_run_dry_run_loop_no_match() {
        // Arrange
        let from_block = 100;
        let to_block = 100;
        let monitor_script = "tx.value > bigint(\"100\")"; // Condition will not be met

        // Create a mock data source
        let mut mock_data_source = MockDataSource::new();
        mock_data_source
            .expect_fetch_block_core_data()
            .with(eq(from_block))
            .times(1)
            .returning(|block_num| {
                let tx = TransactionBuilder::new()
                    .value(U256::from(50)) // Value is less than the script's condition
                    .block_number(block_num)
                    .build();
                let block = BlockBuilder::new().number(block_num).transaction(tx).build();
                Ok((block, vec![]))
            });

        let monitor = Monitor::from_config(
            "Test Monitor".to_string(),
            "testnet".to_string(),
            None,
            None,
            monitor_script.to_string(),
        );

        // Initialize other services
        let abi_service = Arc::new(AbiService::new());
        let block_processor = BlockProcessor::new(abi_service);
        let rhai_config = RhaiConfig::default();
        let rhai_compiler = Arc::new(RhaiCompiler::new(rhai_config.clone()));
        let filtering_engine =
            RhaiFilteringEngine::new(vec![monitor], rhai_compiler, rhai_config);
        let client_pool = Arc::new(HttpClientPool::new());
        let notification_service = NotificationService::new(vec![], client_pool);

        // Act
        let result = run_dry_run_loop(
            from_block,
            to_block,
            Box::new(mock_data_source),
            block_processor,
            filtering_engine,
            notification_service,
        )
        .await;

        // Assert
        assert!(result.is_ok());
        let matches = result.unwrap();
        assert!(matches.is_empty());
    }

    #[tokio::test]
    async fn test_run_dry_run_loop_requires_receipts() {
        // Arrange
        let from_block = 100;
        let to_block = 100;
        let monitor_script = "tx.status == 1"; // Requires receipt data

        // Create a mock data source
        let mut mock_data_source = MockDataSource::new();
        mock_data_source
            .expect_fetch_block_core_data()
            .with(eq(from_block))
            .times(1)
            .returning(|block_num| {
                let tx = TransactionBuilder::new().block_number(block_num).build();
                let block = BlockBuilder::new().number(block_num).transaction(tx).build();
                Ok((block, vec![]))
            });

        // This is the key assertion for this test: fetch_receipts must be called.
        mock_data_source.expect_fetch_receipts().times(1).returning(|_| Ok(Default::default()));

        let monitor = Monitor::from_config(
            "Test Monitor".to_string(),
            "testnet".to_string(),
            None,
            None,
            monitor_script.to_string(),
        );

        // Initialize other services
        let abi_service = Arc::new(AbiService::new());
        let block_processor = BlockProcessor::new(abi_service);
        let rhai_config = RhaiConfig::default();
        let rhai_compiler = Arc::new(RhaiCompiler::new(rhai_config.clone()));
        let filtering_engine =
            RhaiFilteringEngine::new(vec![monitor], rhai_compiler, rhai_config);
        let client_pool = Arc::new(HttpClientPool::new());
        let notification_service = NotificationService::new(vec![], client_pool);

        // Act
        let result = run_dry_run_loop(
            from_block,
            to_block,
            Box::new(mock_data_source),
            block_processor,
            filtering_engine,
            notification_service,
        )
        .await;

        // Assert
        assert!(result.is_ok());
    }
}
