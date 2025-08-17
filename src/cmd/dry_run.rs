use std::sync::Arc;

use clap::Parser;
use thiserror::Error;

use crate::{
    abi::AbiService,
    config::{AppConfig, TriggerLoader, TriggerLoaderError},
    engine::{
        block_processor::{BlockProcessor, BlockProcessorError},
        filtering::RhaiFilteringEngine,
        rhai::RhaiCompiler,
    },
    http_client::HttpClientPool,
    models::monitor_match::MonitorMatch,
    monitor::{MonitorLoader, MonitorLoaderError, MonitorValidationError, MonitorValidator},
    notification::NotificationService,
    providers::{
        rpc::{EvmRpcSource, create_provider},
        traits::DataSourceError,
    },
};

#[derive(Error, Debug)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Config error: {0}")]
    Config(#[from] config::ConfigError),
    #[error("Monitor loading error: {0}")]
    MonitorLoading(#[from] MonitorLoaderError),
    #[error("Trigger loading error: {0}")]
    TriggerLoading(#[from] TriggerLoaderError),
    #[error("Monitor validation error: {0}")]
    MonitorValidation(#[from] MonitorValidationError),
    #[error("Provider error: {0}")]
    Provider(#[from] DataSourceError),
    #[error("Block processor error: {0}")]
    BlockProcessor(#[from] BlockProcessorError),
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Parser, Debug)]
pub struct DryRunArgs {
    /// Path to the monitor file to test.
    #[arg(short, long)]
    monitor: String,
    /// The starting block number.
    #[arg(short, long)]
    from_block: u64,
    /// The ending block number.
    #[arg(short, long)]
    to_block: u64,
    /// Path to the triggers file. If not provided, uses the path from the main
    /// config.
    #[arg(short, long)]
    triggers: Option<String>,
}

pub async fn execute(args: DryRunArgs) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialization
    let config = AppConfig::new(None)?;

    // Init EVM data source
    let provider = create_provider(config.rpc_urls.clone(), config.rpc_retry_config.clone())?;
    let mut evm_source = EvmRpcSource::new(provider);

    // Init ABI service and block processor
    let abi_service = Arc::new(AbiService::new());
    let block_processor = BlockProcessor::new(Arc::clone(&abi_service));

    // 2. Monitor and Trigger Loading
    let monitor_loader = MonitorLoader::new(args.monitor.clone().into());
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
    
    // Init Notification Service
    let client_pool = Arc::new(HttpClientPool::new());
    let notification_service = NotificationService::new(triggers, client_pool);

    // Init Rhai Filtering Engine
    let rhai_compiler = Arc::new(RhaiCompiler::new(config.rhai.clone()));
    let filtering_engine = RhaiFilteringEngine::new(monitors, rhai_compiler, config.rhai.clone());

    tracing::info!("Monitor validation successful.");

    // 4. Core Loop
    let mut matches: Vec<MonitorMatch> = Vec::new();
    let mut current_block = args.from_block;

    tracing::info!(from = args.from_block, to = args.to_block, "Starting block processing...");

    while current_block <= args.to_block {
        unimplemented!();
    }
    tracing::info!("Block processing finished.");

    // 5. Reporting
    let report = serde_json::to_string_pretty(&matches)?;
    println!("{}", report);

    Ok(())
}
