use std::sync::Arc;

use argus::{
    abi::AbiService,
    config::AppConfig,
    initialization::InitializationService,
    persistence::{sqlite::SqliteStateRepository, traits::StateRepository},
    providers::rpc::{create_provider, EvmRpcSource},
    supervisor::Supervisor,
};
use clap::{Parser, Subcommand};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Runs the main monitoring supervisor.
    Run,
    /// Performs a dry run of a single monitor over a specified block range.
    DryRun(DryRunArgs),
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
    /// Path to the triggers file. If not provided, uses the path from the main config.
    #[arg(short, long)]
    triggers: Option<String>,
}

#[tokio::main]
#[tracing::instrument(level = "info")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing subscriber
    let subscriber =
        FmtSubscriber::builder().with_env_filter(EnvFilter::from_default_env()).finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let cli = Cli::parse();

    match cli.command {
        Commands::Run => run_supervisor().await?,
        Commands::DryRun(args) => {
            println!("Dry run command not yet implemented. Args: {:?}", args);
        }
    }

    Ok(())
}

async fn run_supervisor() -> Result<(), Box<dyn std::error::Error>> {
    tracing::debug!("Loading application configuration...");
    let config = AppConfig::new(None)?; // TODO: get config path from env
    tracing::debug!(database_url = %config.database_url, rpc_urls = ?config.rpc_urls, network_id = %config.network_id, "Configuration loaded.");

    tracing::debug!("Initializing state repository...");
    let repo = Arc::new(SqliteStateRepository::new(&config.database_url).await?);
    repo.run_migrations().await?;
    tracing::info!("Database migrations completed.");

    // Initialize ABI service
    tracing::debug!("Initializing ABI service");
    let abi_service = Arc::new(AbiService::new());

    // Initialize application state (monitors, triggers, ABIs) from files into
    // DB/ABI service
    tracing::debug!("Initializing application state...");
    let initialization_service = InitializationService::new(
        config.clone(),
        Arc::clone(&repo) as Arc<dyn StateRepository>,
        Arc::clone(&abi_service),
    );
    initialization_service.run().await?;
    tracing::info!("Application state initialized.");

    tracing::debug!(rpc_urls = ?config.rpc_urls, "Initializing EVM data source...");
    let provider = create_provider(config.rpc_urls.clone(), config.rpc_retry_config.clone())?;
    let evm_data_source = EvmRpcSource::new(provider);
    tracing::info!(retry_policy = ?config.rpc_retry_config, "EVM data source initialized with fallback and retry policy.");

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
