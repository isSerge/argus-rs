use argus::{
    abi::AbiService,
    config::{AppConfig, MonitorLoader, TriggerLoader},
    models::trigger::TriggerConfig,
    persistence::{sqlite::SqliteStateRepository, traits::StateRepository},
    providers::rpc::{create_provider, EvmRpcSource},
    supervisor::Supervisor,
};
use std::{path::PathBuf, sync::Arc};
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
    let repo = Arc::new(SqliteStateRepository::new(&config.database_url).await?);
    repo.run_migrations().await?;
    tracing::info!("Database migrations completed.");

    // Load monitors from the configuration file if specified.
    if let Err(e) = load_monitors_from_file(
        repo.as_ref(),
        &config.network_id,
        &config.monitor_config_path,
    )
    .await
    {
        tracing::error!(error = %e, "Failed to load monitors from file, continuing with monitors already in database (if any).");
    }

    // Load triggers from the configuration file if specified.
    if let Err(e) = load_triggers_from_file(
        repo.as_ref(),
        &config.network_id,
        &config.trigger_config_path,
    )
    .await
    {
        tracing::error!(error = %e, "Failed to load triggers from file, continuing with triggers already in database (if any).");
    }

    tracing::debug!(rpc_urls = ?config.rpc_urls, "Initializing EVM data source...");
    let provider = create_provider(config.rpc_urls.clone(), config.rpc_retry_config.clone())?;
    let evm_data_source = EvmRpcSource::new(provider);
    tracing::info!(retry_policy = ?config.rpc_retry_config, "EVM data source initialized with fallback and retry policy.");

    // Initialize BlockProcessor components
    tracing::debug!("Initializing ABI service");
    let abi_service = Arc::new(AbiService::new());

    let supervisor = Supervisor::builder()
        .config(config)
        .abi_service(abi_service)
        .data_source(Box::new(evm_data_source))
        .state(repo)
        .build()
        .await?;

    tracing::info!("Supervisor initialized, starting monitoring...");

    supervisor.run().await?;

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

async fn load_triggers_from_file(
    repo: &impl StateRepository,
    network_id: &str,
    config_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::debug!(config_path = %config_path, "Loading triggers from configuration file...");
    let trigger_loader = TriggerLoader::new(PathBuf::from(config_path));
    let triggers = trigger_loader.load()?;
    let count = triggers.len();
    tracing::info!(count = count, "Loaded triggers from configuration file.");
    repo.clear_triggers(network_id).await?;
    repo.add_triggers(network_id, triggers).await?;
    tracing::info!(count = count, network_id = %network_id, "Triggers from file stored in database.");
    Ok(())
}
