use std::sync::Arc;

use argus::{
    abi::{AbiService, repository::AbiRepository},
    cmd::{DryRunArgs, dry_run},
    config::{AppConfig, InitialStartBlock},
    engine::rhai::{RhaiCompiler, RhaiScriptValidator},
    initialization::InitializationService,
    persistence::{sqlite::SqliteStateRepository, traits::AppRepository},
    providers::rpc::create_provider,
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
        #[arg(long, value_parser = parse_start_block_arg)]
        from: Option<InitialStartBlock>,
    },
    /// Performs a dry run of a single monitor over a specified block range.
    DryRun(DryRunArgs),
}

/// Parses the start block argument, which can be either "latest" or an integer.
fn parse_start_block_arg(s: &str) -> Result<InitialStartBlock, String> {
    if s.eq_ignore_ascii_case("latest") {
        Ok(InitialStartBlock::Latest)
    } else {
        match s.parse::<i64>() {
            Ok(n) if n >= 0 => Ok(InitialStartBlock::Absolute(n as u64)),
            Ok(n) if n < 0 => Ok(InitialStartBlock::Offset(n)),
            _ => Err(format!("Invalid start block: must be an integer or 'latest', got '{}'", s)),
        }
    }
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
        Commands::Run { config_dir, from } => run_supervisor(config_dir, from).await?,
        Commands::DryRun(args) => dry_run::execute(args).await?,
    }

    Ok(())
}

async fn run_supervisor(
    config_dir: Option<String>,
    from_block_override: Option<InitialStartBlock>,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::debug!("Loading application configuration...");
    let mut config = AppConfig::new(config_dir.as_deref())?;
    tracing::debug!(database_url = %config.database_url, rpc_urls = ?config.rpc_urls, network_id = %config.network_id, "Configuration loaded.");

    // If the CLI flag is provided, it overrides the value from the config file.
    if let Some(override_val) = from_block_override {
        tracing::info!(
            from_block = ?override_val,
            "Overriding initial start block with value from --from-block CLI argument."
        );
        config.initial_start_block = override_val;
    }

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

    // Initialize EVM data provider
    tracing::debug!(rpc_urls = ?config.rpc_urls, "Initializing EVM data provider...");
    let provider =
        Arc::new(create_provider(config.rpc_urls.clone(), config.rpc_retry_config.clone())?);

    // Initialize application state (monitors, triggers, ABIs) from files into
    // DB/ABI service
    tracing::debug!("Initializing application state...");
    let initialization_service = InitializationService::new(
        config.clone(),
        Arc::clone(&repo) as Arc<dyn AppRepository>,
        Arc::clone(&abi_service),
        script_validator,
        provider.clone(),
    );
    initialization_service.run().await?;
    tracing::info!("Application state initialized.");

    let supervisor = Supervisor::builder()
        .config(config)
        .abi_service(abi_service)
        .script_compiler(script_compiler)
        .state(repo)
        .provider(provider)
        .build()
        .await?;

    tracing::info!("Supervisor initialized, starting monitoring...");

    supervisor.run().await?;

    Ok(())
}
