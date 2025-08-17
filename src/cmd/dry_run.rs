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

/// Errors that can occur during dry run execution
#[derive(Error, Debug)]
pub enum DryRunError {
    /// Configuration errors
    #[error("Config error: {0}")]
    Config(#[from] config::ConfigError),

    /// Monitor loading errors
    #[error("Monitor loading error: {0}")]
    MonitorLoading(#[from] MonitorLoaderError),

    /// Trigger loading errors
    #[error("Trigger loading error: {0}")]
    TriggerLoading(#[from] TriggerLoaderError),

    /// Monitor validation errors
    #[error("Monitor validation error: {0}")]
    MonitorValidation(#[from] MonitorValidationError),

    /// Provider errors
    #[error("Provider error: {0}")]
    Provider(#[from] ProviderError),

    /// Data source errors
    #[error("Data source error: {0}")]
    DataSource(#[from] DataSourceError),

    /// Block processor errors
    #[error("Block processor error: {0}")]
    BlockProcessor(#[from] BlockProcessorError),

    /// Filtering engine errors
    #[error("Filtering engine error: {0}")]
    Filtering(#[from] RhaiError),

    /// JSON serialization errors
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Arguments for the dry run command
#[derive(Parser, Debug)]
pub struct DryRunArgs {
    /// Path to the monitor file to test.
    #[arg(short, long)]
    monitor: Option<String>,
    /// The starting block number.
    #[arg(long)]
    from: u64,
    /// The ending block number.
    #[arg(long)]
    to: u64,
    /// Path to the triggers file. If not provided, uses the path from the main
    /// config.
    #[arg(short, long)]
    triggers: Option<String>,
}

/// Executes a dry run of the monitoring process over a specified block range.
pub async fn execute(args: DryRunArgs) -> Result<(), DryRunError> {
    let config = AppConfig::new(None)?;

    // Init EVM data source
    let provider = create_provider(config.rpc_urls.clone(), config.rpc_retry_config.clone())?;
    let evm_source = EvmRpcSource::new(provider);

    // Init ABI service and block processor
    let abi_service = Arc::new(AbiService::new());
    let block_processor = BlockProcessor::new(Arc::clone(&abi_service));

    // Load Monitors and Triggers
    let monitors_path = args.monitor.as_deref().unwrap_or(&config.monitor_config_path);
    let monitor_loader = MonitorLoader::new(monitors_path.into());
    let monitors = monitor_loader.load()?;
    let triggers_path = args.triggers.as_deref().unwrap_or(&config.trigger_config_path);
    let trigger_loader = TriggerLoader::new(triggers_path.into());
    let triggers = trigger_loader.load()?;

    // Monitor Validation
    let monitor_validator = MonitorValidator::new(&config.network_id);
    for monitor in monitors.iter() {
        tracing::debug!(monitor = %monitor.name, "Validating monitor...");
        monitor_validator.validate(&monitor)?;
    }
    tracing::info!("Monitor validation successful.");

    // Init Notification Service
    let client_pool = Arc::new(HttpClientPool::new());
    let notification_service = NotificationService::new(triggers, client_pool);

    // Init Rhai Filtering Engine
    let rhai_compiler = Arc::new(RhaiCompiler::new(config.rhai.clone()));
    let filtering_engine = RhaiFilteringEngine::new(monitors, rhai_compiler, config.rhai.clone());

    // Run the core processing loop
    let matches = run_dry_run_loop(
        args.from,
        args.to,
        Box::new(evm_source),
        block_processor,
        filtering_engine,
        notification_service,
    )
    .await?;

    // Reporting
    let report = serde_json::to_string_pretty(&matches)?;
    println!("{}", report);

    Ok(())
}

/// The core processing loop for the dry run, decoupled from concrete data sources.
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

        let block_data = BlockData::from_raw_data(block, receipts, logs);
        let decoded_blocks = block_processor.process_blocks_batch(vec![block_data]).await?;

        for decoded_block in decoded_blocks {
            for item in decoded_block.items {
                let item_matches = filtering_engine.evaluate_item(&item).await?;
                for m in item_matches {
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
