use argus::{
    config::AppConfig,
    data_source::{DataSource, EvmRpcSource},
    models::BlockData,
    provider::create_provider,
    state::{SqliteStateRepository, StateRepository},
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
    let provider = create_provider(config.rpc_urls, config.retry_config.clone())?;
    let evm_data_source = EvmRpcSource::new(provider);
    tracing::info!(retry_policy = ?config.retry_config, "EVM data source initialized with fallback and retry policy.");

    tracing::info!(network_id = %config.network_id, "Starting EVM monitor.");

    // Main monitoring loop
    loop {
        tracing::trace!("Starting new monitoring cycle.");
        match monitor_cycle(
            &repo,
            &evm_data_source,
            &config.network_id,
            config.block_chunk_size,
            config.confirmation_blocks,
        )
        .await
        {
            Ok(_) => {
                tracing::trace!("Monitoring cycle completed successfully.");
            }
            Err(e) => {
                tracing::error!(error = %e, "Error in monitoring cycle.");
                // Continue monitoring even if there's an error
            }
        }

        // Wait for the configured polling interval before the next cycle
        tokio::time::sleep(Duration::from_millis(config.polling_interval_ms)).await;
    }
}

#[tracing::instrument(skip(repo, data_source), level = "debug")]
async fn monitor_cycle(
    repo: &SqliteStateRepository,
    data_source: &impl DataSource,
    network_id: &str,
    block_chunk_size: u64,
    confirmation_blocks: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Read the last processed block from the StateRepository
    tracing::debug!(network_id = %network_id, "Fetching last processed block from state repository.");
    let last_processed_block = repo.get_last_processed_block(network_id).await?;
    tracing::debug!(network_id = %network_id, last_processed_block = ?last_processed_block, "Last processed block retrieved.");

    // 2. Determine a target "to block"
    tracing::debug!(network_id = %network_id, "Fetching current block number from data source.");
    let current_block = data_source.get_current_block_number().await?;
    tracing::debug!(network_id = %network_id, current_block = %current_block, "Current block number retrieved.");

    // Check if the chain is long enough to handle the confirmation buffer
    if current_block < confirmation_blocks {
        tracing::info!(
            network_id = %network_id,
            current_block = %current_block,
            confirmation_blocks = %confirmation_blocks,
            "Chain is shorter than the confirmation buffer. Waiting for more blocks."
        );
        return Ok(());
    }

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

    let safe_to_block = current_block.saturating_sub(confirmation_blocks);

    // Don't process if we're already caught up to the safe block
    if from_block > safe_to_block {
        tracing::info!(
            network_id = %network_id,
            current_block = %current_block,
            from_block = %from_block,
            "Caught up to confirmation buffer. Waiting for more blocks."
        );
        return Ok(());
    }

    // Process in smaller chunks to avoid hitting RPC limits
    let to_block = std::cmp::min(from_block + block_chunk_size, safe_to_block);

    tracing::info!(
        network_id = %network_id,
        from_block = %from_block,
        to_block = %to_block,
        "Processing block range."
    );

    // 3. Process each block in the range
    for block_num in from_block..=to_block {
        match data_source.fetch_block_core_data(block_num).await {
            Ok((block, logs)) => {
                let block_data =
                    BlockData::from_raw_data(block, std::collections::HashMap::new(), logs);

                // TODO: Implement block processing logic here.
                // This will involve:
                // 1. Analyzing the `block_data` against user-defined rules.
                // 2. Based on the analysis, determining if any transaction receipts are needed.
                // 3. If so, calling a method on the data_source to fetch only the required receipts,
                //    and adding them to `block_data`.
                // 4. Passing the `block_data` to the trigger evaluation engine.

                let tx_count = block_data.block.transactions.len();
                let log_count = block_data.logs.values().map(Vec::len).sum::<usize>();
                let block_hash = block_data.block.header.hash;

                tracing::info!(
                    network_id = %network_id,
                    block_number = block_num,
                    block_hash = %block_hash,
                    tx_count = tx_count,
                    log_count = log_count,
                    "Processed block."
                );
            }
            Err(e) => {
                // If fetching a block fails, we log the error and stop the current
                // cycle. The last processed block will not be updated, ensuring
                // that we retry the failed block in the next cycle.
                tracing::error!(
                    network_id = %network_id,
                    block_number = block_num,
                    error = %e,
                    "Failed to process block. Aborting cycle."
                );
                return Err(e.into());
            }
        }
    }

    // 4. Update the StateRepository with the new last processed block number
    tracing::debug!(network_id = %network_id, new_last_processed_block = %to_block, "Updating last processed block in state repository.");
    repo.set_last_processed_block(network_id, to_block).await?;
    tracing::info!(network_id = %network_id, last_processed_block = %to_block, "Last processed block updated successfully.");

    Ok(())
}
