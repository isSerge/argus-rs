use argus::{
    config::AppConfig,
    data_source::{DataSource, new_http_source},
    state::{SqliteStateRepository, StateRepository},
};
use tokio::time::{Duration, sleep};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = AppConfig::new()?;

    let repo = SqliteStateRepository::new(&config.database_url).await?;
    repo.run_migrations().await?;

    let evm_data_source = new_http_source(&config.rpc_urls[0])?;

    println!("Starting EVM monitor for network: {}", config.network_id);

    // Main monitoring loop
    loop {
        match monitor_cycle(&repo, &evm_data_source, &config.network_id).await {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Error in monitoring cycle: {e}");
                // Continue monitoring even if there's an error
            }
        }

        // Wait 10 seconds before the next cycle
        sleep(Duration::from_secs(10)).await;
    }
}

async fn monitor_cycle(
    repo: &SqliteStateRepository,
    data_source: &impl DataSource,
    network_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Read the last processed block from the StateRepository
    let last_processed_block = repo.get_last_processed_block(network_id).await?;

    // 2. Determine a target "to block"
    let current_block = data_source.get_current_block_number().await?;

    let from_block = match last_processed_block {
        Some(block) => block + 1, // Start from the next block after the last processed
        None => {
            // If no blocks have been processed, start from a recent block (e.g., current - 100)
            // to avoid processing the entire blockchain history on first run
            current_block.saturating_sub(100)
        }
    };

    // Don't process if we're already caught up
    if from_block > current_block {
        println!(
            "Already caught up. Current block: {current_block}, last processed: {last_processed_block:?}"
        );
        return Ok(());
    }

    // Process in smaller chunks to avoid hitting RPC limits
    // Use 5 blocks per chunk to be conservative with RPC limits
    let to_block = std::cmp::min(from_block + 5, current_block);

    println!("Processing blocks {from_block} to {to_block} (current: {current_block})");

    // 3. Call data_source.fetch_logs() with the block range
    let logs = data_source.fetch_logs(from_block, to_block).await?;

    // 4. Print the number of logs found to the console
    println!(
        "Found {} logs in blocks {} to {}",
        logs.len(),
        from_block,
        to_block
    );

    // 5. Update the StateRepository with the new last processed block number
    repo.set_last_processed_block(network_id, to_block).await?;

    Ok(())
}
