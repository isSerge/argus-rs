//! This module provides functionality to execute a dry run of the monitoring
//! process over a specified block range.

use std::{collections::HashMap, sync::Arc};

use alloy::primitives;
use clap::Parser;
use dashmap::DashMap;
use thiserror::Error;

use crate::{
    abi::{AbiError, AbiRepository, AbiService, repository::AbiRepositoryError},
    actions::{ActionDispatcher, error::ActionDispatcherError},
    config::AppConfig,
    engine::{
        alert_manager::{AlertManager, AlertManagerError},
        block_processor::process_blocks_batch,
        filtering::{FilteringEngine, RhaiError, RhaiFilteringEngine},
        rhai::{RhaiCompiler, RhaiScriptValidator},
    },
    http_client::HttpClientPool,
    loader::{LoaderError, load_config},
    models::{
        BlockData,
        action::{ActionConfig, ActionConfigError},
        monitor::MonitorConfig,
        monitor_match::MonitorMatch,
    },
    monitor::{MonitorManager, MonitorValidationError, MonitorValidator},
    persistence::{error::PersistenceError, sqlite::SqliteStateRepository, traits::AppRepository},
    providers::{
        rpc::{EvmRpcSource, ProviderError, create_provider},
        traits::{DataSource, DataSourceError},
    },
};

/// Errors that can occur during the execution of a dry run.
#[derive(Error, Debug)]
pub enum DryRunError {
    /// An error occurred while loading the application configuration.
    #[error("Config error: {0}")]
    Config(#[from] config::ConfigError),

    /// An error occurred while loading monitor definitions.
    #[error("Monitor loading error: {0}")]
    MonitorLoading(#[from] LoaderError),

    /// An error occurred while loading action definitions.
    #[error("Action loading error: {0}")]
    ActionLoading(#[from] ActionConfigError),

    /// A monitor failed validation against the defined rules.
    #[error("Monitor validation error: {0}")]
    MonitorValidation(#[from] MonitorValidationError),

    /// An error occurred with the blockchain provider.
    #[error("Provider error: {0}")]
    Provider(#[from] ProviderError),

    /// An error occurred while fetching data from the blockchain.
    #[error("Data source error: {0}")]
    DataSource(#[from] DataSourceError),

    /// An error occurred during the block processing stage.
    #[error("Block processor error: {0}")]
    BlockProcessor(#[from] Box<dyn std::error::Error + Send + Sync>),

    /// An error occurred within the filtering engine, likely during script
    /// execution.
    #[error("Filtering engine error: {0}")]
    Filtering(#[from] RhaiError),

    /// An error occurred while serializing the final report to JSON.
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    /// An error occurred while interacting with the ABI repository.
    #[error("ABI repository error: {0}")]
    AbiRepository(#[from] AbiRepositoryError),

    /// An error occurred in the alert manager.
    #[error("Alert manager error: {0}")]
    AlertManager(#[from] AlertManagerError),

    /// An error occurred in the state repository.
    #[error("State repository error: {0}")]
    StateRepository(#[from] PersistenceError),

    /// An error occurred in the ABI service.
    #[error("ABI service error: {0}")]
    AbiServiceError(#[from] AbiError),

    /// An error occurred with the block range specified for the dry run.
    #[error("Invalid block range: {from} - {to}")]
    InvalidBlockRange {
        /// The starting block number.
        from: u64,
        /// The ending block number.
        to: u64,
    },

    /// An error occurred in the action dispatcher.
    #[error("Action dispatcher error: {0}")]
    ActionDispatcher(#[from] ActionDispatcherError),
}

/// A command to perform a dry run of monitors over a specified block range.
///
/// This command initializes the application's services in a one-shot mode to
/// test monitor configurations against historical blockchain data. It fetches,
/// processes, and filters data for each block in the range, dispatches real
/// notifications for any matches, and prints a final summary report of all
/// matches to standard output.
#[derive(Parser, Debug)]
pub struct DryRunArgs {
    /// Path to configuration directory containing app, monitor and action
    /// configs
    #[arg(short, long)]
    config_dir: Option<String>,
    /// The starting block number for the dry run (inclusive).
    #[arg(long)]
    from: u64,
    /// The ending block number for the dry run (inclusive).
    #[arg(long)]
    to: u64,
}

/// The main entry point for the `dry-run` command.
///
/// This function orchestrates the entire dry run process:
/// 1. Loads the main application configuration.
/// 2. Initializes all necessary services (data source, block processor,
///    filtering engine, etc.).
/// 3. Loads and validates the monitor and action configurations.
/// 4. Calls `run_dry_run_loop` to execute the core processing logic.
/// 5. Serializes the results to a pretty JSON string and prints to stdout.
pub async fn execute(args: DryRunArgs) -> Result<(), DryRunError> {
    let config_dir = args.config_dir.as_deref().unwrap_or("configs");
    let config = AppConfig::new(config_dir.into())?;

    // Init services for processing and decoding data.
    let abi_repository = Arc::new(AbiRepository::new(&config.abi_config_path)?);
    let abi_service = Arc::new(AbiService::new(abi_repository));

    // Init Rhai scripting engine and validator
    let rhai_compiler = Arc::new(RhaiCompiler::new(config.rhai.clone()));
    let script_validator = RhaiScriptValidator::new(rhai_compiler.clone());

    // Load and validate monitor and action configurations from files.
    let monitors = load_config::<MonitorConfig>(config.monitor_config_path)?;
    let actions = load_config::<ActionConfig>(config.action_config_path)?;

    // Init a temporary, in-memory state repository for the dry run.
    let state_repo = Arc::new(SqliteStateRepository::new("sqlite::memory:").await?);
    state_repo.run_migrations().await?;

    // Link ABIs for monitors that require them.
    for monitor in monitors.iter() {
        if let (Some(address_str), Some(abi_name)) = (&monitor.address, &monitor.abi) {
            if address_str.eq_ignore_ascii_case("all") {
                abi_service.add_global_abi(abi_name)?;
            } else {
                let address: primitives::Address = address_str.parse().map_err(|_| {
                    DryRunError::MonitorValidation(MonitorValidationError::InvalidAddress {
                        monitor_name: monitor.name.clone(),
                        address: address_str.clone(),
                    })
                })?;
                abi_service.link_abi(address, abi_name)?;
            }
        }
    }

    let monitor_validator =
        MonitorValidator::new(script_validator, abi_service.clone(), &config.network_id, &actions);
    for monitor in monitors.iter() {
        tracing::debug!(monitor = %monitor.name, "Validating monitor...");
        monitor_validator.validate(monitor)?;
    }
    tracing::info!("Monitor validation successful.");

    let count = monitors.len();
    tracing::info!(count = count, "Loaded monitors from configuration file.");
    // Store monitors in the temporary state repository for access during processing
    // (simulates default mode behavior).
    state_repo.clear_monitors(&config.network_id).await?;
    state_repo.add_monitors(&config.network_id, monitors).await?;
    tracing::info!(count = count, network_id = %&config.network_id, "Monitors from file stored in database.");
    let monitors = state_repo.get_monitors(&config.network_id).await?;
    tracing::info!(count = monitors.len(), "Loaded monitors from database for dry run.");

    let monitor_manager =
        Arc::new(MonitorManager::new(monitors.clone(), rhai_compiler.clone(), abi_service.clone()));

    // Init EVM data source for fetching blockchain data.
    let provider =
        Arc::new(create_provider(config.rpc_urls.clone(), config.rpc_retry_config.clone())?);
    let evm_source = EvmRpcSource::new(provider, monitor_manager.clone());

    // Init services for notifications and filtering logic.
    let client_pool = Arc::new(HttpClientPool::new(config.http_base_config.clone()));
    let actions: Arc<HashMap<String, ActionConfig>> =
        Arc::new(actions.into_iter().map(|t| (t.name.clone(), t)).collect());
    let action_dispatcher = Arc::new(ActionDispatcher::new(actions.clone(), client_pool).await?);
    let filtering_engine = RhaiFilteringEngine::new(
        abi_service.clone(),
        rhai_compiler,
        config.rhai.clone(),
        monitor_manager.clone(),
    );

    // Init the AlertManager with the in-memory state repository.
    let alert_manager = Arc::new(AlertManager::new(action_dispatcher, state_repo, actions));

    // Execute the core processing loop.
    let matches = run_dry_run_loop(
        args.from,
        args.to,
        Box::new(evm_source),
        filtering_engine,
        alert_manager.clone(),
    )
    .await?;

    // Get actual dispatch statistics from AlertManager
    let dispatched_notifications = alert_manager.get_dispatched_notifications();

    // Print the summary report.
    print_summary_report(args.from, args.to, &matches, dispatched_notifications);

    Ok(())
}

/// Prints a summary report of the dry run results.
fn print_summary_report(
    from_block: u64,
    to_block: u64,
    matches: &[MonitorMatch],
    dispatched_notifications: &DashMap<String, usize>,
) {
    let total_blocks = to_block - from_block + 1;
    let total_matches = matches.len();

    let mut matches_by_monitor: HashMap<String, usize> = HashMap::new();

    for m in matches {
        *matches_by_monitor.entry(m.monitor_name.clone()).or_insert(0) += 1;
    }

    println!("\nDry Run Report");
    println!("==============");

    println!("\nSummary");
    println!("-------");
    println!("- Blocks Processed: {} to {} ({} blocks)", from_block, to_block, total_blocks);
    println!("- Total Matches Found: {}", total_matches);

    println!("\nMatches by Monitor");
    println!("------------------");
    if matches_by_monitor.is_empty() {
        println!("- No matches found.");
    } else {
        for (monitor_name, count) in &matches_by_monitor {
            println!("- \"{}\": {}", monitor_name, count);
        }
    }

    println!("\nNotifications Dispatched");
    println!("------------------------");
    if dispatched_notifications.is_empty() {
        println!("- No notifications dispatched.");
    } else {
        for entry in dispatched_notifications.iter() {
            println!("- \"{}\": {}", entry.key(), entry.value());
        }
    }
}

/// The core processing logic for the dry run.
///
/// # Arguments
///
/// * `from_block` - The starting block number.
/// * `to_block` - The ending block number.
/// * `data_source` - A boxed trait object for fetching blockchain data.
/// * `block_processor` - The service for decoding raw block data.
/// * `filtering_engine` - The service for evaluating data against monitor
///   scripts.
/// * `alert_manager` - The service for managing alerts and notifications.
///
/// # Returns
///
/// A `Result` containing a vector of all `MonitorMatch`es found during the run,
/// or a `DryRunError`.
async fn run_dry_run_loop(
    from_block: u64,
    to_block: u64,
    data_source: Box<dyn DataSource>,
    filtering_engine: RhaiFilteringEngine,
    alert_manager: Arc<AlertManager<SqliteStateRepository>>,
) -> Result<Vec<MonitorMatch>, DryRunError> {
    // Define a batch size for processing blocks in chunks.
    const BATCH_SIZE: u64 = 50;

    let mut matches: Vec<MonitorMatch> = Vec::new();
    let mut current_block = from_block;

    tracing::info!(
        from = from_block,
        to = to_block,
        batch_size = BATCH_SIZE,
        "Starting block processing..."
    );

    // Check if block sequence is valid
    if from_block > to_block {
        return Err(DryRunError::InvalidBlockRange { from: from_block, to: to_block });
    }

    // The outer loop now iterates over batches of blocks.
    while current_block <= to_block {
        let batch_end_block = (current_block + BATCH_SIZE - 1).min(to_block);
        tracing::info!(from = current_block, to = batch_end_block, "Fetching block batch...");

        let mut block_data_batch =
            Vec::with_capacity((batch_end_block - current_block + 1) as usize);

        // Inner loop collects data for the current batch.
        for block_num in current_block..=batch_end_block {
            let (block, logs) = data_source.fetch_block_core_data(block_num).await?;

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
            block_data_batch.push(BlockData::from_raw_data(block, receipts, logs));
        }

        // Process the entire collected batch in one call.
        if !block_data_batch.is_empty() {
            tracing::info!(count = block_data_batch.len(), "Processing block batch...");
            let decoded_blocks_batch = process_blocks_batch(block_data_batch).await?;

            // Evaluate each item from the entire batch of decoded blocks.
            for decoded_block in decoded_blocks_batch {
                for item in decoded_block.items {
                    let item_matches = filtering_engine.evaluate_item(&item).await?;
                    let mut limited_matches = item_matches.clone();
                    if limited_matches.len() > 10 {
                        limited_matches.truncate(10);
                    }
                    for m in limited_matches {
                        alert_manager.process_match(&m).await?;
                        matches.push(m.clone());
                    }
                }
            }
        }

        // Move to the next batch.
        current_block = batch_end_block + 1;
    }
    tracing::info!("Block processing finished.");

    alert_manager.flush().await?;
    tracing::info!("Dispatched any pending aggregated notifications.");

    Ok(matches)
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use alloy::primitives::U256;
    use mockall::predicate::eq;
    use tempfile::tempdir;

    use super::*;
    use crate::{
        abi::{AbiRepository, AbiService},
        actions::ActionDispatcher,
        config::RhaiConfig,
        engine::{alert_manager::AlertManager, filtering::RhaiFilteringEngine, rhai::RhaiCompiler},
        http_client::HttpClientPool,
        models::monitor_match::{MatchData, TransactionMatchData},
        persistence::sqlite::SqliteStateRepository,
        providers::traits::MockDataSource,
        test_helpers::{ActionBuilder, BlockBuilder, MonitorBuilder, TransactionBuilder},
    };

    // A helper function to create an AlertManager with a mock state repository.
    async fn create_test_alert_manager(
        actions: Arc<HashMap<String, ActionConfig>>,
    ) -> Arc<AlertManager<SqliteStateRepository>> {
        let state_repo = SqliteStateRepository::new("sqlite::memory:")
            .await
            .expect("Failed to connect to in-memory db");
        state_repo.run_migrations().await.expect("Failed to run migrations");
        let client_pool = Arc::new(HttpClientPool::default());
        let action_dispatcher =
            Arc::new(ActionDispatcher::new(actions.clone(), client_pool).await.unwrap());
        Arc::new(AlertManager::new(action_dispatcher, Arc::new(state_repo), actions))
    }

    #[tokio::test]
    async fn test_run_dry_run_loop_succeeds() {
        // Arrange
        let from_block = 100;
        let to_block = 100;
        let monitor_script = "tx.value > bigint(\"10000000000000000000\")";

        // Create a mock data source
        let mut mock_data_source = MockDataSource::new();
        mock_data_source.expect_fetch_block_core_data().with(eq(from_block)).times(1).returning(
            |block_num| {
                let tx = TransactionBuilder::new().value(U256::MAX).block_number(block_num).build();
                let block = BlockBuilder::new().number(block_num).transaction(tx).build();
                Ok((block, vec![]))
            },
        );

        // Create a monitor that will match the transaction
        let monitor = MonitorBuilder::new()
            .filter_script(monitor_script)
            .actions(vec!["test-action".to_string()])
            .build();

        // Initialize other services
        let temp_dir = tempdir().unwrap();
        let abi_repo = Arc::new(AbiRepository::new(temp_dir.path()).unwrap());
        let abi_service = Arc::new(AbiService::new(abi_repo));
        let rhai_config = RhaiConfig::default();
        let rhai_compiler = Arc::new(RhaiCompiler::new(rhai_config.clone()));
        let monitors = vec![monitor];
        let monitor_manager = Arc::new(MonitorManager::new(
            monitors.clone(),
            rhai_compiler.clone(),
            abi_service.clone(),
        ));
        let filtering_engine = RhaiFilteringEngine::new(
            abi_service.clone(),
            rhai_compiler,
            rhai_config,
            monitor_manager.clone(),
        );
        let actions = HashMap::from([(
            "test-action".to_string(),
            ActionBuilder::new("test-action")
                .slack_config(
                    "https://hooks.slack.com/services/T00000000/B00000000/XXXXXXXXXXXXXXXXXXXXXXXX",
                )
                .build(),
        )]);
        let alert_manager = create_test_alert_manager(Arc::new(actions)).await;

        // Act
        let result = run_dry_run_loop(
            from_block,
            to_block,
            Box::new(mock_data_source),
            filtering_engine,
            alert_manager,
        )
        .await;

        // Assert
        assert!(result.is_ok());
        let matches = result.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].block_number, from_block);
        assert!(matches!(
            matches[0].match_data,
            MatchData::Transaction(TransactionMatchData { .. })
        ));
    }

    #[tokio::test]
    async fn test_run_run_loop_no_match() {
        // Arrange
        let from_block = 100;
        let to_block = 100;
        let monitor_script = "tx.value > bigint(\"100\")"; // Condition will not be met

        // Create a mock data source
        let mut mock_data_source = MockDataSource::new();
        mock_data_source.expect_fetch_block_core_data().with(eq(from_block)).times(1).returning(
            |block_num| {
                let tx = TransactionBuilder::new()
                    .value(U256::from(50)) // Value is less than the script's condition
                    .block_number(block_num)
                    .build();
                let block = BlockBuilder::new().number(block_num).transaction(tx).build();
                Ok((block, vec![]))
            },
        );

        let monitor = MonitorBuilder::new().filter_script(monitor_script).build();

        // Initialize other services
        let temp_dir = tempdir().unwrap();
        let abi_repo = Arc::new(AbiRepository::new(temp_dir.path()).unwrap());
        let abi_service = Arc::new(AbiService::new(abi_repo));
        let rhai_config = RhaiConfig::default();
        let rhai_compiler = Arc::new(RhaiCompiler::new(rhai_config.clone()));
        let monitors = vec![monitor];
        let monitor_manager = Arc::new(MonitorManager::new(
            monitors.clone(),
            rhai_compiler.clone(),
            abi_service.clone(),
        ));
        let filtering_engine = RhaiFilteringEngine::new(
            abi_service.clone(),
            rhai_compiler,
            rhai_config,
            monitor_manager.clone(),
        );
        let alert_manager = create_test_alert_manager(Arc::new(HashMap::new())).await;

        // Act
        let result = run_dry_run_loop(
            from_block,
            to_block,
            Box::new(mock_data_source),
            filtering_engine,
            alert_manager,
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
        mock_data_source.expect_fetch_block_core_data().with(eq(from_block)).times(1).returning(
            |block_num| {
                let tx = TransactionBuilder::new().block_number(block_num).build();
                let block = BlockBuilder::new().number(block_num).transaction(tx).build();
                Ok((block, vec![]))
            },
        );

        // This is the key assertion for this test: fetch_receipts must be called.
        mock_data_source.expect_fetch_receipts().times(1).returning(|_| Ok(Default::default()));

        let monitor = MonitorBuilder::new().filter_script(monitor_script).build();

        // Initialize other services
        let temp_dir = tempdir().unwrap();
        let abi_repo = Arc::new(AbiRepository::new(temp_dir.path()).unwrap());
        let abi_service = Arc::new(AbiService::new(abi_repo));
        let rhai_config = RhaiConfig::default();
        let rhai_compiler = Arc::new(RhaiCompiler::new(rhai_config.clone()));
        let monitors = vec![monitor];
        let monitor_manager = Arc::new(MonitorManager::new(
            monitors.clone(),
            rhai_compiler.clone(),
            abi_service.clone(),
        ));
        let filtering_engine = RhaiFilteringEngine::new(
            abi_service.clone(),
            rhai_compiler,
            rhai_config,
            monitor_manager.clone(),
        );
        let alert_manager = create_test_alert_manager(Arc::new(HashMap::new())).await;

        // Act
        let result = run_dry_run_loop(
            from_block,
            to_block,
            Box::new(mock_data_source),
            filtering_engine,
            alert_manager,
        )
        .await;

        // Assert
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_run_dry_run_loop_data_source_error() {
        // Arrange
        let from_block = 100;
        let to_block = 100;
        let monitor_script = "tx.value > 100";

        // Create a mock data source that returns an error
        let mut mock_data_source = MockDataSource::new();
        mock_data_source
            .expect_fetch_block_core_data()
            .with(eq(from_block))
            .times(1)
            .returning(|_| Err(DataSourceError::BlockNotFound(100)));

        let monitor = MonitorBuilder::new().filter_script(monitor_script).build();

        // Initialize other services
        let temp_dir = tempdir().unwrap();
        let abi_repo = Arc::new(AbiRepository::new(temp_dir.path()).unwrap());
        let abi_service = Arc::new(AbiService::new(abi_repo));
        let rhai_config = RhaiConfig::default();
        let rhai_compiler = Arc::new(RhaiCompiler::new(rhai_config.clone()));
        let monitors = vec![monitor];
        let monitor_manager = Arc::new(MonitorManager::new(
            monitors.clone(),
            rhai_compiler.clone(),
            abi_service.clone(),
        ));
        let filtering_engine = RhaiFilteringEngine::new(
            abi_service.clone(),
            rhai_compiler,
            rhai_config,
            monitor_manager.clone(),
        );
        let alert_manager = create_test_alert_manager(Arc::new(HashMap::new())).await;

        // Act
        let result = run_dry_run_loop(
            from_block,
            to_block,
            Box::new(mock_data_source),
            filtering_engine,
            alert_manager,
        )
        .await;

        // Assert
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, DryRunError::DataSource(_)));
    }

    #[tokio::test]
    async fn test_run_dry_run_loop_multiple_blocks() {
        // Arrange
        let from_block = 100;
        let to_block = 101;
        let monitor_script = "tx.value > bigint(\"10000000000000000000\")";

        // Create a mock data source
        let mut mock_data_source = MockDataSource::new();

        // Block 100 - with matching tx
        mock_data_source.expect_fetch_block_core_data().with(eq(100)).times(1).returning(
            |block_num| {
                let tx = TransactionBuilder::new().value(U256::MAX).block_number(block_num).build();
                let block = BlockBuilder::new().number(block_num).transaction(tx).build();
                Ok((block, vec![]))
            },
        );

        // Block 101 - with non-matching tx
        mock_data_source.expect_fetch_block_core_data().with(eq(101)).times(1).returning(
            |block_num| {
                let tx =
                    TransactionBuilder::new().value(U256::from(10)).block_number(block_num).build();
                let block = BlockBuilder::new().number(block_num).transaction(tx).build();
                Ok((block, vec![]))
            },
        );

        // Create a monitor that will match the transaction
        let monitor = MonitorBuilder::new()
            .filter_script(monitor_script)
            .actions(vec!["test-action".to_string()])
            .build();

        // Initialize other services
        let temp_dir = tempdir().unwrap();
        let abi_repo = Arc::new(AbiRepository::new(temp_dir.path()).unwrap());
        let abi_service = Arc::new(AbiService::new(abi_repo));
        let rhai_config = RhaiConfig::default();
        let rhai_compiler = Arc::new(RhaiCompiler::new(rhai_config.clone()));
        let monitors = vec![monitor];
        let monitor_manager = Arc::new(MonitorManager::new(
            monitors.clone(),
            rhai_compiler.clone(),
            abi_service.clone(),
        ));
        let filtering_engine = RhaiFilteringEngine::new(
            abi_service.clone(),
            rhai_compiler,
            rhai_config,
            monitor_manager.clone(),
        );
        let actions = HashMap::from([(
            "test-action".to_string(),
            ActionBuilder::new("test-action")
                .slack_config(
                    "https://hooks.slack.com/services/T00000000/B00000000/XXXXXXXXXXXXXXXXXXXXXXXX",
                )
                .build(),
        )]);
        let alert_manager = create_test_alert_manager(Arc::new(actions)).await;

        // Act
        let result = run_dry_run_loop(
            from_block,
            to_block,
            Box::new(mock_data_source),
            filtering_engine,
            alert_manager,
        )
        .await;

        // Assert
        assert!(result.is_ok());
        let matches = result.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].block_number, 100);
    }

    #[tokio::test]
    async fn test_run_dry_run_loop_invalid_block_range() {
        // Arrange
        let from_block = 101;
        let to_block = 100; // Invalid range

        // Create a mock data source (should not be called)
        let mock_data_source = MockDataSource::new();

        // Initialize other services with minimal setup
        let temp_dir = tempdir().unwrap();
        let abi_repo = Arc::new(AbiRepository::new(temp_dir.path()).unwrap());
        let abi_service = Arc::new(AbiService::new(abi_repo));
        let rhai_config = RhaiConfig::default();
        let rhai_compiler = Arc::new(RhaiCompiler::new(rhai_config.clone()));
        let monitors = vec![];
        let monitor_manager = Arc::new(MonitorManager::new(
            monitors.clone(),
            rhai_compiler.clone(),
            abi_service.clone(),
        ));
        let filtering_engine = RhaiFilteringEngine::new(
            abi_service.clone(),
            rhai_compiler,
            rhai_config,
            monitor_manager.clone(),
        );
        let alert_manager = create_test_alert_manager(Arc::new(HashMap::new())).await;

        // Act
        let result = run_dry_run_loop(
            from_block,
            to_block,
            Box::new(mock_data_source),
            filtering_engine,
            alert_manager,
        )
        .await;

        // Assert
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, DryRunError::InvalidBlockRange { from: 101, to: 100 }));
    }
}
