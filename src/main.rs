use std::sync::Arc;

use argus::{
    abi::{AbiService, repository::AbiRepository},
    cmd::{DryRunArgs, dry_run},
    config::{AppConfig, ActionConfig},
    engine::rhai::{RhaiCompiler, RhaiScriptValidator},
    initialization::InitializationService,
    loader::load_config,
    persistence::{sqlite::SqliteStateRepository, traits::StateRepository},
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
    Run {
        #[arg(short, long)]
        config_dir: Option<String>,
    },
    /// Performs a dry run of a single monitor over a specified block range.
    DryRun(DryRunArgs),
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
        Commands::Run { config_dir } => run_supervisor(config_dir).await?,
        Commands::DryRun(args) => dry_run::execute(args).await?,
    }

    Ok(())
}

async fn run_supervisor(config_dir: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    tracing::debug!("Loading application configuration...");
    let config = AppConfig::new(config_dir.as_deref())?;
    tracing::debug!(database_url = %config.database_url, rpc_urls = ?config.rpc_urls, network_id = %config.network_id, "Configuration loaded.");

    tracing::debug!("Initializing state repository...");
    let repo = Arc::new(SqliteStateRepository::new(&config.database_url).await?);
    repo.run_migrations().await?;
    tracing::info!("Database migrations completed.");

    // Initialize ABI repository
    tracing::debug!("Initializing ABI repository...");
    let abi_repository = Arc::new(AbiRepository::new(&config.abi_config_path)?);
    tracing::info!("ABI repository initialized with {} ABIs.", abi_repository.len());

    // Initialize ABI service
    tracing::debug!("Initializing ABI service");
    let abi_service = Arc::new(AbiService::new(Arc::clone(&abi_repository)));

    // Initialize script validator
    let script_compiler = Arc::new(RhaiCompiler::new(config.rhai.clone()));
    let script_validator = RhaiScriptValidator::new(script_compiler.clone());

    // Initialize application state (monitors, triggers, ABIs) from files into
    // DB/ABI service
    tracing::debug!("Initializing application state...");
    let initialization_service = InitializationService::new(
        config.clone(),
        Arc::clone(&repo) as Arc<dyn StateRepository>,
        Arc::clone(&abi_service),
        script_validator,
    );
    initialization_service.run().await?;
    tracing::info!("Application state initialized.");

    // Load actions configuration
    tracing::debug!("Loading actions configuration...");
    let actions = load_config::<ActionConfig>(config.actions_config_path.clone(), false)?;
    tracing::info!(count = actions.len(), "Actions configuration loaded.");

    let supervisor = Supervisor::builder()
        .config(config)
        .abi_service(abi_service)
        .script_compiler(script_compiler)
        .state(repo)
        .actions(actions)
        .build()
        .await?;

    tracing::info!("Supervisor initialized, starting monitoring...");

    supervisor.run().await?;

    Ok(())
}
