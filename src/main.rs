use argus::{
    config::AppConfig,
    data_source::{DataSource, EvmRpcSource},
    state::{SqliteStateRepository, StateRepository},
    provider::{create_provider, RetryBackoff},
};
use tokio::time::Duration;
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

    tracing::debug!(rpc_urls = ?config.rpc_urls, "Initializing resilient EVM data source...");
    let retry_config = RetryBackoff::default();
    let provider = create_provider(config.rpc_urls, retry_config.clone())?;
    let evm_data_source = EvmRpcSource::new(provider);
    tracing::info!(retry_policy = ?retry_config, "EVM data source initialized with fallback and retry policy.");

    tracing::info!(network_id = %config.network_id, "Starting EVM monitor.");

    // Main monitoring loop
    loop {
        tracing::trace!("Starting new monitoring cycle.");
        match monitor_cycle(&repo, &evm_data_source, &config.network_id).await {
            Ok(_) => {
                tracing::trace!("Monitoring cycle completed successfully.");
            }
            Err(e) => {
                tracing::error!(error = %e, "Error in monitoring cycle.");
                // Continue monitoring even if there's an error
            }
        }

        // Wait 10 seconds before the next cycle
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

#[tracing::instrument(skip(repo, data_source), level = "debug")]
async fn monitor_cycle(
    repo: &SqliteStateRepository,
    data_source: &impl DataSource,
    network_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Read the last processed block from the StateRepository
    tracing::debug!(network_id = %network_id, "Fetching last processed block from state repository.");
    let last_processed_block = repo.get_last_processed_block(network_id).await?;
    tracing::debug!(network_id = %network_id, last_processed_block = ?last_processed_block, "Last processed block retrieved.");

    // 2. Determine a target "to block"
    tracing::debug!(network_id = %network_id, "Fetching current block number from data source.");
    let current_block = data_source.get_current_block_number().await?;
    tracing::debug!(network_id = %network_id, current_block = %current_block, "Current block number retrieved.");

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

    // Don't process if we're already caught up
    if from_block > current_block {
        tracing::info!(
            network_id = %network_id,
            current_block = %current_block,
            last_processed_block = ?last_processed_block,
            "Already caught up. No new blocks to process."
        );
        return Ok(());
    }

    // Process in smaller chunks to avoid hitting RPC limits
    // Using 5 blocks per chunk to be conservative with RPC limits
    let to_block = std::cmp::min(from_block + 5, current_block);

    tracing::info!(
        network_id = %network_id,
        from_block = %from_block,
        to_block = %to_block,
        current_block = %current_block,
        "Processing block range."
    );

    // 3. Call data_source.fetch_logs() with the block range
    tracing::debug!(network_id = %network_id, from_block = %from_block, to_block = %to_block, "Fetching logs for block range.");
    let logs = data_source.fetch_logs(from_block, to_block).await?;

    // 4. Log the number of logs found
    tracing::info!(
        network_id = %network_id,
        log_count = logs.len(),
        from_block = %from_block,
        to_block = %to_block,
        "Found logs in block range."
    );

    // 5. Update the StateRepository with the new last processed block number
    tracing::debug!(network_id = %network_id, new_last_processed_block = %to_block, "Updating last processed block in state repository.");
    repo.set_last_processed_block(network_id, to_block).await?;
    tracing::info!(network_id = %network_id, last_processed_block = %to_block, "Last processed block updated successfully.");

    Ok(())
}
