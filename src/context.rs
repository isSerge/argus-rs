//! Application context and initialization logic.
//! This module handles loading configuration, setting up the database,
//! initializing the ABI service, and preparing the EVM data provider.
//! The `AppContext` struct encapsulates all these components for use
//! throughout the application.

use std::{path::PathBuf, sync::Arc};

use alloy::providers::Provider;
use thiserror::Error;

use crate::{
    abi::{repository::{AbiRepository, AbiRepositoryError}, AbiService},
    config::{AppConfig, InitialStartBlock},
    engine::rhai::{RhaiCompiler, RhaiScriptValidator},
    loader::load_config,
    models::{action::ActionConfig, monitor::MonitorConfig},
    monitor::MonitorValidator,
    persistence::{error::PersistenceError, sqlite::SqliteStateRepository, traits::AppRepository},
    providers::rpc::{create_provider, ProviderError},
};

/// Errors that can occur during application context initialization.
#[derive(Debug, Error)]
pub enum AppContextError {
    /// Configuration error.
    #[error("Config error: {0}")]
    Config(#[from] config::ConfigError),

    /// Persistence error.
    #[error("Persistence error: {0}")]
    Persistence(#[from] PersistenceError),

    /// ABI repository error.
    #[error("ABI repository error: {0}")]
    AbiRepository(#[from] AbiRepositoryError),

    /// Provider error.
    #[error("Provider error: {0}")]
    Provider(#[from] ProviderError),

    /// Initialization error.
    #[error("Initialization error: {0}")]
    Initialization(#[from] InitializationError),
}

/// Errors that can occur during specific initialization steps.
#[derive(Debug, Error)]
pub enum InitializationError {
    /// Failed to load monitors from file.
    #[error("Failed to load monitors from file: {0}")]
    MonitorLoad(String),

    /// Failed to load action from file.
    #[error("Failed to load action from file: {0}")]
    ActionLoad(String),

    /// Failed to load ABIs from monitors.
    #[error("Failed to load ABIs from monitors: {0}")]
    AbiLoad(String),

    /// Failed to initialize block state.
    #[error("Failed to initialize block state: {0}")]
    BlockStateInitialization(String),
}

/// The application context, holding configuration, database repository,
/// ABI service, script compiler, and EVM data provider.
pub struct AppContext<T: AppRepository> {
    /// Shared application configuration.
    pub config: AppConfig,

    /// The state repository for database interactions.
    pub repo: Arc<T>,

    /// The ABI service for managing and querying ABIs.
    pub abi_service: Arc<AbiService>,

    /// The Rhai script compiler for compiling and validating scripts.
    pub script_compiler: Arc<RhaiCompiler>,

    /// The EVM data provider for blockchain interactions.
    pub provider: Arc<dyn Provider + Send + Sync>,
}

/// A builder for the `AppContext`, allowing configuration overrides
/// and step-by-step initialization.
pub struct AppContextBuilder {
    /// Optional configuration directory to load settings from.
    config_dir: Option<String>,

    /// Optional override for the initial start block.
    from_block_override: Option<InitialStartBlock>,

    /// Optional override for the database URL.
    database_url_override: Option<String>,
}

impl AppContextBuilder {
    /// Creates a new `AppContextBuilder` with optional configuration directory
    /// and initial start block override.
    pub fn new(config_dir: Option<String>, from_block_override: Option<InitialStartBlock>) -> Self {
        Self { config_dir, from_block_override, database_url_override: None }
    }

    /// Sets a database URL override.
    pub fn database_url(mut self, url: String) -> Self {
        self.database_url_override = Some(url);
        self
    }

    /// Builds the `AppContext`, performing all initialization steps.
    /// This includes loading configuration, setting up the database,
    /// initializing the ABI service, and preparing the EVM data provider.
    pub async fn build(self) -> Result<AppContext<SqliteStateRepository>, AppContextError> {
        tracing::debug!("Loading application configuration...");
        let mut config = AppConfig::new(self.config_dir.as_deref())?;
        tracing::debug!(database_url = %config.database_url, rpc_urls = ?config.rpc_urls, network_id = %config.network_id, "Configuration loaded.");

        if let Some(override_val) = self.from_block_override {
            tracing::info!(
                from_block = ?override_val,
                "Overriding initial start block with value from --from-block CLI argument."
            );
            config.initial_start_block = override_val;
        }

        if let Some(db_url) = self.database_url_override {
            tracing::info!(
                database_url = %db_url,
                "Overriding database URL."
            );
            config.database_url = db_url;
        }

        tracing::debug!("Initializing state repository...");
        let repo = Arc::new(SqliteStateRepository::new(&config.database_url).await?);
        repo.run_migrations().await?;
        tracing::info!("Database migrations completed.");

        tracing::debug!("Initializing ABI repository...");
        let abi_repository = Arc::new(AbiRepository::new(&config.abi_config_path)?);
        tracing::info!("ABI repository initialized with {} ABIs.", abi_repository.len());

        tracing::debug!("Initializing ABI service");
        let abi_service = Arc::new(AbiService::new(Arc::clone(&abi_repository)));

        let script_compiler = Arc::new(RhaiCompiler::new(config.rhai.clone()));

        tracing::debug!(rpc_urls = ?config.rpc_urls, "Initializing EVM data provider...");
        let provider =
            Arc::new(create_provider(config.rpc_urls.clone(), config.rpc_retry_config.clone())?);

        Self::initialize_block_state(&config, repo.as_ref(), provider.as_ref()).await?;
        Self::load_actions_from_file(&config, repo.as_ref()).await?;
        Self::load_monitors_from_file(
            &config,
            repo.as_ref(),
            abi_service.clone(),
            script_compiler.clone(),
        )
        .await?;
        Self::load_abis_from_monitors(&config, repo.as_ref(), abi_service.clone()).await?;

        Ok(AppContext { config, repo, abi_service, script_compiler, provider })
    }

    /// Initializes the block state in the database if not already set.
    /// This sets the latest processed block based on the configuration
    /// and the current block number from the provider.
    async fn initialize_block_state(
        config: &AppConfig,
        repo: &dyn AppRepository,
        provider: &dyn Provider,
    ) -> Result<(), InitializationError> {
        let network_id = &config.network_id;

        let latest_block = repo.get_last_processed_block(network_id).await.map_err(|e| {
            InitializationError::BlockStateInitialization(format!(
                "Failed to fetch latest processed block from DB: {e}"
            ))
        })?;

        if latest_block.is_some() {
            tracing::info!(network_id = %network_id, "Latest processed block already set in database. Skipping block state initialization.");
            return Ok(());
        }

        tracing::info!(network_id = %network_id, "No latest processed block found in database. Initializing...");
        let latest_block_number = provider.get_block_number().await.map_err(|e| {
            InitializationError::BlockStateInitialization(format!(
                "Failed to fetch latest block number from provider: {e}"
            ))
        })?;

        let safe_head = latest_block_number.saturating_sub(config.confirmation_blocks);

        let target_block = match config.initial_start_block {
            InitialStartBlock::Latest => latest_block_number,
            InitialStartBlock::Absolute(n) => n,
            InitialStartBlock::Offset(offset) => latest_block_number.saturating_add_signed(offset),
        };

        let final_start_block = std::cmp::min(target_block, safe_head);

        tracing::info!(
            network_id = %network_id,
            latest_block = latest_block_number,
            confirmation_blocks = config.confirmation_blocks,
            safe_head = safe_head,
            initial_start_block = ?config.initial_start_block,
            start_block = final_start_block,
            "Setting latest processed block in database."
        );

        repo.set_last_processed_block(network_id, final_start_block.saturating_sub(1))
            .await
            .map_err(|e| {
                InitializationError::BlockStateInitialization(format!(
                    "Failed to set latest processed block in DB: {e}"
                ))
            })?;

        Ok(())
    }

    /// Loads monitors from the configuration file into the database
    /// if no monitors currently exist in the database. Validates each
    /// monitor before storing it.
    async fn load_monitors_from_file(
        config: &AppConfig,
        repo: &dyn AppRepository,
        abi_service: Arc<AbiService>,
        script_compiler: Arc<RhaiCompiler>,
    ) -> Result<(), InitializationError> {
        let network_id = &config.network_id;
        let config_path = &config.monitor_config_path;

        tracing::debug!(network_id = %network_id, "Checking for existing monitors in database...");
        let existing_monitors = repo.get_monitors(network_id).await.map_err(|e| {
            InitializationError::MonitorLoad(format!(
                "Failed to fetch existing monitors from DB: {e}"
            ))
        })?;

        if !existing_monitors.is_empty() {
            tracing::info!(
                network_id = %network_id,
                count = existing_monitors.len(),
                "Monitors already exist in the database. Skipping loading from file."
            );
            return Ok(());
        }

        tracing::info!(config_path = %config_path.display(), "No monitors found in database. Loading from configuration file...");
        let monitors = load_config::<MonitorConfig>(PathBuf::from(config_path)).map_err(|e| {
            InitializationError::MonitorLoad(format!("Failed to load monitors from file: {e}"))
        })?;

        let actions = repo.get_actions(network_id).await.map_err(|e| {
            InitializationError::MonitorLoad(format!(
                "Failed to fetch actions for monitor validation: {e}"
            ))
        })?;

        let script_validator = RhaiScriptValidator::new(script_compiler);
        let validator = MonitorValidator::new(script_validator, abi_service, network_id, &actions);
        for monitor in &monitors {
            validator.validate(monitor).map_err(|e| {
                InitializationError::MonitorLoad(format!(
                    "Monitor validation failed for '{}': {}",
                    monitor.name, e
                ))
            })?;
        }

        let count = monitors.len();
        tracing::info!(count = count, "Loaded monitors from configuration file.");
        repo.clear_monitors(network_id).await.map_err(|e| {
            InitializationError::MonitorLoad(format!(
                "Failed to clear existing monitors in DB: {e}"
            ))
        })?;
        repo.add_monitors(network_id, monitors).await.map_err(|e| {
            InitializationError::MonitorLoad(format!("Failed to add monitors to DB: {e}"))
        })?;
        tracing::info!(count = count, network_id = %network_id, "Monitors from file stored in database.");
        Ok(())
    }

    /// Loads actions from the configuration file into the database
    /// if no actions currently exist in the database. Validates each
    /// action during loading.
    async fn load_actions_from_file(
        config: &AppConfig,
        repo: &dyn AppRepository,
    ) -> Result<(), InitializationError> {
        let network_id = &config.network_id;
        let config_path = &config.action_config_path;

        tracing::debug!(network_id = %network_id, "Checking for existing actions in database...");
        let existing_actions = repo.get_actions(network_id).await.map_err(|e| {
            InitializationError::ActionLoad(format!(
                "Failed to fetch existing actions from DB: {e}"
            ))
        })?;

        if !existing_actions.is_empty() {
            tracing::info!(
                network_id = %network_id,
                count = existing_actions.len(),
                "actions already exist in the database. Skipping loading from file."
            );
            return Ok(());
        }

        tracing::info!(config_path = %config_path.display(), "No actions found in database. Loading from configuration file...");
        let actions = load_config::<ActionConfig>(PathBuf::from(config_path)).map_err(|e| {
            InitializationError::ActionLoad(format!("Failed to load actions from file: {e}"))
        })?;
        let count = actions.len();
        tracing::info!(count = count, "Loaded actions from configuration file.");
        repo.clear_actions(network_id).await.map_err(|e| {
            InitializationError::ActionLoad(format!("Failed to clear existing actions in DB: {e}"))
        })?;
        repo.add_actions(network_id, actions).await.map_err(|e| {
            InitializationError::ActionLoad(format!("Failed to add actions to DB: {e}"))
        })?;
        tracing::info!(count = count, network_id = %network_id, "actions from file stored in database.");
        Ok(())
    }

    /// Loads ABIs referenced by monitors in the database into the ABI service.
    /// This ensures that all ABIs needed for monitoring are available.
    async fn load_abis_from_monitors(
        config: &AppConfig,
        repo: &dyn AppRepository,
        abi_service: Arc<AbiService>,
    ) -> Result<(), InitializationError> {
        let network_id = &config.network_id;
        tracing::debug!(network_id = %network_id, "Loading ABIs from monitors in database...");
        let monitors = repo.get_monitors(network_id).await.map_err(|e| {
            InitializationError::AbiLoad(format!("Failed to fetch monitors for ABI loading: {e}"))
        })?;

        for monitor in &monitors {
            if let (Some(address_str), Some(abi_name)) = (&monitor.address, &monitor.abi) {
                if address_str.eq_ignore_ascii_case("all") {
                    abi_service.add_global_abi(abi_name).map_err(|e| {
                        InitializationError::AbiLoad(format!(
                            "Failed to add global ABI '{}' for monitor '{}': {}",
                            abi_name, monitor.name, e
                        ))
                    })?;
                } else {
                    let address = address_str.parse().map_err(|e| {
                        InitializationError::AbiLoad(format!(
                            "Failed to parse address for monitor '{}': {}",
                            monitor.name, e
                        ))
                    })?;

                    abi_service.link_abi(address, abi_name).map_err(|e| {
                        InitializationError::AbiLoad(format!(
                            "Failed to link ABI '{}' for monitor '{}': {}",
                            abi_name, monitor.name, e
                        ))
                    })?;
                }
            }
        }
        tracing::info!(count = monitors.len(), network_id = %network_id, "ABIs loaded for monitors from database.");
        Ok(())
    }
}
