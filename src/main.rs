use argus::{
    cmd::{DryRunArgs, dry_run},
    config::InitialStartBlock,
    context::AppContextBuilder,
    persistence::SqliteStateRepository,
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

/// Parses the start block argument, which can be either "latest", positive or
/// negative integer.
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
    let context = AppContextBuilder::new(config_dir, from_block_override).build().await?;

    let supervisor =
        Supervisor::<SqliteStateRepository>::builder().context(context).build().await?;

    tracing::info!("Supervisor initialized, starting monitoring...");

    supervisor.run().await?;

    Ok(())
}
