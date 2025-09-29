//! This module provides the `InitializationService` responsible for loading
//! initial application data (monitors, actions, ABIs) into the database and
//! ABI service at startup.

use std::{path::PathBuf, sync::Arc};

use alloy::providers::Provider;
use thiserror::Error;

use crate::{
    abi::AbiService,
    config::{AppConfig, InitialStartBlock},
    engine::rhai::RhaiScriptValidator,
    loader::load_config,
    models::{action::ActionConfig, monitor::MonitorConfig},
    monitor::MonitorValidator,
    persistence::traits::AppRepository,
};

/// Errors that can occur during initialization.
#[derive(Debug, Error)]
pub enum InitializationError {
    /// An error occurred while loading monitors from the configuration file.
    #[error("Failed to load monitors from file: {0}")]
    MonitorLoad(String),

    /// An error occurred while loading action from the configuration file.
    #[error("Failed to load action from file: {0}")]
    ActionLoad(String),

    /// An error occurred while loading ABIs from monitors.
    #[error("Failed to load ABIs from monitors: {0}")]
    AbiLoad(String),

    /// An error occurred while initializing the block state.
    #[error("Failed to initialize block state: {0}")]
    BlockStateInitialization(String),
}

/// A service responsible for initializing application state at startup.
pub struct InitializationService {
    config: AppConfig,
    repo: Arc<dyn AppRepository>,
    abi_service: Arc<AbiService>,
    script_validator: RhaiScriptValidator,
    provider: Arc<dyn Provider + Send + Sync>,
}

impl InitializationService {
    /// Creates a new `InitializationService`.
    pub fn new(
        config: AppConfig,
        repo: Arc<dyn AppRepository>,
        abi_service: Arc<AbiService>,
        script_validator: RhaiScriptValidator,
        provider: Arc<dyn Provider + Send + Sync>,
    ) -> Self {
        Self { config, repo, abi_service, script_validator, provider }
    }

    /// Runs the initialization process, loading monitors, actions, and ABIs.
    pub async fn run(&self) -> Result<(), InitializationError> {
        self.initialize_block_state().await?;

        // Load actions from the configuration file if specified and DB is empty.
        self.load_actions_from_file().await?;

        // Load monitors from the configuration file if specified and DB is empty.
        self.load_monitors_from_file().await?;

        // Load ABIs into the AbiService from the monitors stored in the database.
        self.load_abis_from_monitors().await?;

        Ok(())
    }

    async fn initialize_block_state(&self) -> Result<(), InitializationError> {
        let network_id = &self.config.network_id;

        // Check if the latest processed block is already set
        let latest_block = self.repo.get_last_processed_block(network_id).await.map_err(|e| {
            InitializationError::BlockStateInitialization(format!(
                "Failed to fetch latest processed block from DB: {e}"
            ))
        })?;

        if latest_block.is_some() {
            tracing::info!(network_id = %network_id, "Latest processed block already set in database. Skipping block state initialization.");
            return Ok(());
        }

        // No block found, initialize it
        tracing::info!(network_id = %network_id, "No latest processed block found in database. Initializing...");
        let latest_block_number = self.provider.get_block_number().await.map_err(|e| {
            InitializationError::BlockStateInitialization(format!(
                "Failed to fetch latest block number from provider: {e}"
            ))
        })?;

        let safe_head = latest_block_number.saturating_sub(self.config.confirmation_blocks);

        let target_block = match self.config.initial_start_block {
            InitialStartBlock::Latest => latest_block_number,
            InitialStartBlock::Absolute(n) => n,
            InitialStartBlock::Offset(offset) => latest_block_number.saturating_add_signed(offset),
        };

        let final_start_block = std::cmp::min(target_block, safe_head);

        // Set the latest processed block in the database
        tracing::info!(
            network_id = %network_id,
            latest_block = latest_block_number,
            confirmation_blocks = self.config.confirmation_blocks,
            safe_head = safe_head,
            initial_start_block = ?self.config.initial_start_block,
            start_block = final_start_block,
            "Setting latest processed block in database."
        );

        // Set to one less than the start block so processing begins from the start
        // block
        self.repo
            .set_last_processed_block(network_id, final_start_block.saturating_sub(1))
            .await
            .map_err(|e| {
            InitializationError::BlockStateInitialization(format!(
                "Failed to set latest processed block in DB: {e}"
            ))
        })?;

        Ok(())
    }

    pub(crate) async fn load_monitors_from_file(&self) -> Result<(), InitializationError> {
        let network_id = &self.config.network_id;
        let config_path = &self.config.monitor_config_path;

        tracing::debug!(network_id = %network_id, "Checking for existing monitors in database...");
        let existing_monitors = self.repo.get_monitors(network_id).await.map_err(|e| {
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

        // Validate monitors
        let actions = self.repo.get_actions(network_id).await.map_err(|e| {
            InitializationError::MonitorLoad(format!(
                "Failed to fetch actions for monitor validation: {e}"
            ))
        })?;

        let validator = MonitorValidator::new(
            self.script_validator.clone(),
            self.abi_service.clone(),
            network_id,
            &actions,
        );
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
        self.repo.clear_monitors(network_id).await.map_err(|e| {
            InitializationError::MonitorLoad(format!(
                "Failed to clear existing monitors in DB: {e}"
            ))
        })?;
        self.repo.add_monitors(network_id, monitors).await.map_err(|e| {
            InitializationError::MonitorLoad(format!("Failed to add monitors to DB: {e}"))
        })?;
        tracing::info!(count = count, network_id = %network_id, "Monitors from file stored in database.");
        Ok(())
    }

    pub(crate) async fn load_actions_from_file(&self) -> Result<(), InitializationError> {
        let network_id = &self.config.network_id;
        let config_path = &self.config.action_config_path;

        tracing::debug!(network_id = %network_id, "Checking for existing actions in database...");
        let existing_actions = self.repo.get_actions(network_id).await.map_err(|e| {
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
        self.repo.clear_actions(network_id).await.map_err(|e| {
            InitializationError::ActionLoad(format!("Failed to clear existing actions in DB: {e}"))
        })?;
        self.repo.add_actions(network_id, actions).await.map_err(|e| {
            InitializationError::ActionLoad(format!("Failed to add actions to DB: {e}"))
        })?;
        tracing::info!(count = count, network_id = %network_id, "actions from file stored in database.");
        Ok(())
    }

    pub(crate) async fn load_abis_from_monitors(&self) -> Result<(), InitializationError> {
        let network_id = &self.config.network_id;
        tracing::debug!(network_id = %network_id, "Loading ABIs from monitors in database...");
        let monitors = self.repo.get_monitors(network_id).await.map_err(|e| {
            InitializationError::AbiLoad(format!("Failed to fetch monitors for ABI loading: {e}"))
        })?;

        for monitor in &monitors {
            if let (Some(address_str), Some(abi_name)) = (&monitor.address, &monitor.abi) {
                if address_str.eq_ignore_ascii_case("all") {
                    self.abi_service.add_global_abi(abi_name).map_err(|e| {
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

                    self.abi_service.link_abi(address, abi_name).map_err(|e| {
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

#[cfg(test)]
mod tests {
    use std::fs;

    use alloy::primitives::{U256, address};
    use mockall::predicate::*;
    use tempfile::tempdir;

    use super::*;
    use crate::{
        config::{AppConfig, RhaiConfig},
        engine::rhai::RhaiCompiler,
        models::{action::ActionConfig, monitor::MonitorConfig},
        persistence::{error::PersistenceError, traits::MockAppRepository},
        test_helpers::{ActionBuilder, MonitorBuilder, create_test_abi_service, mock_provider},
    };

    // Helper to create a dummy config file
    fn create_dummy_config_file(dir: &tempfile::TempDir, filename: &str, content: &str) -> PathBuf {
        let file_path = dir.path().join(filename);
        fs::write(&file_path, content).unwrap();
        file_path
    }

    fn create_test_action_config_str() -> &'static str {
        r#"
actions:
  - name: "Test Action"
    webhook:
      url: "http://example.com"
      message:
        title: "Test Title"
        body: "Test Body"
"#
    }

    fn create_test_monitor_config_str() -> &'static str {
        r#"
monitors:
  - name: "Test Monitor"
    network: "testnet"
    address: "0x0000000000000000000000000000000000000123"
    abi: "test_abi"
    filter_script: "log.name == 'A'"
"#
    }

    fn create_test_abi_content() -> &'static str {
        r#"
[
    {
        "type": "function",
        "name": "balanceOf",
        "inputs": [
            {"name": "account", "type": "address"}
        ],
        "outputs": [
            {"name": "", "type": "uint256"}
        ]
    }
]
"#
    }

    fn create_script_validator() -> RhaiScriptValidator {
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config));
        RhaiScriptValidator::new(compiler)
    }

    #[tokio::test]
    async fn test_run_happy_path_db_empty() {
        let (provider, asserter) = mock_provider();
        let temp_dir = tempdir().unwrap();
        let monitor_config_path =
            create_dummy_config_file(&temp_dir, "monitors.yaml", create_test_monitor_config_str());
        let action_config_path =
            create_dummy_config_file(&temp_dir, "actions.yaml", create_test_action_config_str());
        let network_id = "testnet";

        let mut mock_repo = MockAppRepository::new();

        // Monitors
        mock_repo
            .expect_get_monitors()
            .with(eq(network_id))
            .times(2) // Called once for monitor loading, once for ABI loading
            .returning(|_| Ok(vec![]));
        mock_repo.expect_clear_monitors().with(eq(network_id)).once().returning(|_| Ok(()));
        mock_repo
            .expect_add_monitors()
            .with(eq(network_id), always())
            .once()
            .returning(|_, _| Ok(()));

        // actions
        mock_repo
            .expect_get_actions()
            .with(eq(network_id))
            .times(2) // Called for action loading and monitor validation
            .returning(|_| Ok(vec![]));
        mock_repo.expect_clear_actions().with(eq(network_id)).once().returning(|_| Ok(()));
        mock_repo
            .expect_add_actions()
            .with(eq(network_id), always())
            .once()
            .returning(|_, _| Ok(()));

        // Block state
        asserter.push_success(&U256::from(10));
        mock_repo
            .expect_get_last_processed_block()
            .with(eq(network_id))
            .once()
            .returning(|_| Ok(None));
        mock_repo
            .expect_set_last_processed_block()
            .with(eq(network_id), eq(4)) // 10 - 5 (confirmation blocks) - 1
            .once()
            .returning(|_, _| Ok(()));

        let config = AppConfig::builder()
            .network_id(network_id)
            .monitor_config_path(monitor_config_path.to_str().unwrap())
            .action_config_path(action_config_path.to_str().unwrap())
            .abi_config_path(temp_dir.path().to_str().unwrap())
            .initial_start_block(InitialStartBlock::Offset(-5)) // 5 blocks behind latest
            .build();

        let (abi_service, _) =
            create_test_abi_service(&temp_dir, &[("test_abi", create_test_abi_content())]);
        // Link the ABI for the test monitor
        abi_service
            .link_abi("0x0000000000000000000000000000000000000123".parse().unwrap(), "test_abi")
            .unwrap();
        let script_validator = create_script_validator();
        let initialization_service = InitializationService::new(
            config,
            Arc::new(mock_repo),
            abi_service,
            script_validator,
            provider,
        );

        let result = initialization_service.run().await;

        assert!(result.is_ok(), "Expected run to succeed, but got error: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_run_skips_loading_if_db_not_empty() {
        let (provider, _asserter) = mock_provider();
        let network_id = "testnet";

        let monitor = MonitorBuilder::new().network(network_id).name("Existing Monitor").build();

        let trigger =
            ActionBuilder::new("Existing Trigger").webhook_config("http://example.com").build();

        let mut mock_repo = MockAppRepository::new();
        // Return existing monitors
        mock_repo
            .expect_get_monitors()
            .with(eq(network_id))
            .times(2) // Called once for monitor loading, once for ABI loading
            .returning(move |_| Ok(vec![monitor.clone()]));

        // Return existing actions
        mock_repo
            .expect_get_actions()
            .with(eq(network_id))
            .once()
            .returning(move |_| Ok(vec![trigger.clone()]));

        // Block state
        mock_repo
            .expect_get_last_processed_block()
            .with(eq(network_id))
            .once()
            .returning(|_| Ok(Some(100)));

        // Ensure file loading is NOT called
        mock_repo.expect_add_monitors().times(0);
        mock_repo.expect_add_actions().times(0);
        mock_repo.expect_set_last_processed_block().times(0);

        let config = AppConfig::builder().network_id(network_id).build();
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Dummy path, won't be used
        let script_validator = create_script_validator();
        let initialization_service = InitializationService::new(
            config,
            Arc::new(mock_repo),
            abi_service,
            script_validator,
            provider,
        );

        let result = initialization_service.run().await;
        assert!(result.is_ok(), "Expected run to succeed, but got error: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_load_monitors_from_file_repo_error() {
        let (provider, _asserter) = mock_provider();
        let temp_dir = tempdir().unwrap();
        let config_path = create_dummy_config_file(&temp_dir, "monitors.yaml", "monitors: []");
        let network_id = "testnet";

        let mut mock_repo = MockAppRepository::new();
        mock_repo
            .expect_get_monitors()
            .with(eq(network_id))
            .once()
            .returning(|_| Err(PersistenceError::NotFound)); // Simulate a DB error

        let config = AppConfig::builder()
            .network_id(network_id)
            .monitor_config_path(config_path.to_str().unwrap())
            .build();

        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Dummy path, won't be used
        let script_validator = create_script_validator();
        let initialization_service = InitializationService::new(
            config,
            Arc::new(mock_repo),
            abi_service,
            script_validator,
            provider,
        );

        let result = initialization_service.load_monitors_from_file().await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            InitializationError::MonitorLoad(msg) if msg.contains("Failed to fetch existing monitors")
        ));
    }

    #[tokio::test]
    async fn test_load_abis_from_monitors_invalid_address() {
        let (provider, _asserter) = mock_provider();
        let network_id = "testnet";

        let monitor = MonitorBuilder::new()
            .network(network_id)
            .name("ABI Monitor")
            .address("not-a-valid-address")
            .abi("test_abi")
            .build();

        let mut mock_repo = MockAppRepository::new();
        mock_repo
            .expect_get_monitors()
            .with(eq(network_id))
            .once()
            .returning(move |_| Ok(vec![monitor.clone()]));

        let config = AppConfig::builder().network_id(network_id).build();
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Dummy path, won't be used
        let script_validator = create_script_validator();
        let initialization_service = InitializationService::new(
            config,
            Arc::new(mock_repo),
            abi_service,
            script_validator,
            provider,
        );

        let result = initialization_service.load_abis_from_monitors().await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            InitializationError::AbiLoad(msg) if msg.contains("Failed to parse address")
        ));
    }

    #[tokio::test]
    async fn test_load_abis_from_monitors_abi_not_found_in_repo() {
        let (provider, _asserter) = mock_provider();
        let temp_dir = tempdir().unwrap();
        // No ABI file created, so repository will be empty
        let network_id = "testnet";
        let monitor = MonitorBuilder::new()
            .name("ABI Monitor")
            .network(network_id)
            .address("0x0000000000000000000000000000000000000001")
            .abi("non_existent_abi")
            .build();

        let mut mock_repo = MockAppRepository::new();
        mock_repo
            .expect_get_monitors()
            .with(eq(network_id))
            .once()
            .returning(move |_| Ok(vec![monitor.clone()]));

        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Empty repo

        let config = AppConfig::builder().network_id(network_id).build();
        let script_validator = create_script_validator();
        let initialization_service = InitializationService::new(
            config,
            Arc::new(mock_repo),
            abi_service,
            script_validator,
            provider,
        );

        let result = initialization_service.load_abis_from_monitors().await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            InitializationError::AbiLoad(msg) if msg.contains("Failed to link ABI 'non_existent_abi'")
        ));
    }

    #[tokio::test]
    async fn test_load_monitors_from_file_when_db_empty() {
        let (provider, _asserter) = mock_provider();
        let temp_dir = tempdir().unwrap();
        let config_path =
            create_dummy_config_file(&temp_dir, "monitors.yaml", create_test_monitor_config_str());
        let network_id = "testnet";

        let mut mock_repo = MockAppRepository::new();
        // Expect get_monitors to be called and return empty
        mock_repo.expect_get_monitors().with(eq(network_id)).once().returning(|_| Ok(vec![]));
        // Expect get_actions to be called for validation
        mock_repo.expect_get_actions().with(eq(network_id)).once().returning(|_| Ok(vec![]));
        // Expect clear_monitors and add_monitors to be called
        mock_repo.expect_clear_monitors().with(eq(network_id)).once().returning(|_| Ok(()));
        mock_repo
            .expect_add_monitors()
            .with(eq(network_id), function(|monitors: &Vec<MonitorConfig>| monitors.len() == 1))
            .once()
            .returning(|_, _| Ok(()));

        // Dummy config for AppConfig
        let config = AppConfig::builder()
            .network_id(network_id)
            .monitor_config_path(config_path.to_str().unwrap())
            .abi_config_path(temp_dir.path().to_str().unwrap())
            .build();

        let (abi_service, _) =
            create_test_abi_service(&temp_dir, &[("test_abi", create_test_abi_content())]);
        // Link the ABI for the test monitor
        abi_service
            .link_abi(address!("0x0000000000000000000000000000000000000123"), "test_abi")
            .unwrap();
        let script_validator = create_script_validator();
        let initialization_service = InitializationService::new(
            config,
            Arc::new(mock_repo),
            abi_service,
            script_validator,
            provider,
        );

        let result = initialization_service.load_monitors_from_file().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_load_monitors_from_file_when_db_empty_invalid_monitor() {
        let (provider, _asserter) = mock_provider();
        let temp_dir = tempdir().unwrap();
        let config_path = create_dummy_config_file(
            &temp_dir,
            "monitors.yaml",
            r#"
monitors:
  - name: "Invalid Monitor"
    network: "testnet"
    address: "0x0000000000000000000000000000000000000001"
    filter_script: "log.name == 'A'"
"#, // Invalid: no ABI provided
        );
        let network_id = "testnet";

        let mut mock_repo = MockAppRepository::new();
        // Expect get_monitors to be called and return empty
        mock_repo.expect_get_monitors().with(eq(network_id)).once().returning(|_| Ok(vec![]));
        // Expect get_actions to be called for validation
        mock_repo.expect_get_actions().with(eq(network_id)).once().returning(|_| Ok(vec![]));

        // Dummy config for AppConfig
        let config = AppConfig::builder()
            .network_id(network_id)
            .monitor_config_path(config_path.to_str().unwrap())
            .build();

        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Dummy path, won't be used
        let script_validator = create_script_validator();
        let initialization_service = InitializationService::new(
            config,
            Arc::new(mock_repo),
            abi_service,
            script_validator,
            provider,
        );

        let result = initialization_service.load_monitors_from_file().await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            InitializationError::MonitorLoad(msg) if msg.contains("Monitor validation failed")
        ));
    }

    #[tokio::test]
    async fn test_load_monitors_from_file_when_db_not_empty() {
        let (provider, _asserter) = mock_provider();
        let temp_dir = tempdir().unwrap();
        let config_path =
            create_dummy_config_file(&temp_dir, "monitors.yaml", create_test_monitor_config_str());
        let network_id = "testnet";
        let monitor = MonitorBuilder::new().network(network_id).name("Dummy Monitor").build();

        let mut mock_repo = MockAppRepository::new();
        // Expect get_monitors to be called and return non-empty
        mock_repo
            .expect_get_monitors()
            .with(eq(network_id))
            .once()
            .returning(move |_| Ok(vec![monitor.clone()])); // Return a dummy monitor
        // Expect clear_monitors and add_monitors to NOT be called
        mock_repo.expect_clear_monitors().times(0);
        mock_repo.expect_add_monitors().times(0);

        // Dummy config for AppConfig
        let config = AppConfig::builder()
            .network_id(network_id)
            .monitor_config_path(config_path.to_str().unwrap())
            .build();

        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Dummy path, won't be used
        let script_validator = create_script_validator();
        let initialization_service = InitializationService::new(
            config,
            Arc::new(mock_repo),
            abi_service,
            script_validator,
            provider,
        );

        let result = initialization_service.load_monitors_from_file().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_load_actions_from_file_when_db_empty() {
        let (provider, _asserter) = mock_provider();
        let temp_dir = tempdir().unwrap();
        let config_path =
            create_dummy_config_file(&temp_dir, "actions.yaml", create_test_action_config_str());
        let network_id = "testnet";

        let mut mock_repo = MockAppRepository::new();
        // Expect get_actions to be called and return empty
        mock_repo.expect_get_actions().with(eq(network_id)).once().returning(|_| Ok(vec![]));
        // Expect clear_actions and add_actions to be called
        mock_repo.expect_clear_actions().with(eq(network_id)).once().returning(|_| Ok(()));
        mock_repo
            .expect_add_actions()
            .with(eq(network_id), function(|actions: &Vec<ActionConfig>| actions.len() == 1))
            .once()
            .returning(|_, _| Ok(()));

        // Dummy config for AppConfig
        let config = AppConfig::builder()
            .network_id(network_id)
            .action_config_path(config_path.to_str().unwrap())
            .build();

        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Dummy path, won't be used
        let script_validator = create_script_validator();
        let initialization_service = InitializationService::new(
            config,
            Arc::new(mock_repo),
            abi_service,
            script_validator,
            provider,
        );

        let result = initialization_service.load_actions_from_file().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_load_actions_from_file_when_db_not_empty() {
        let (provider, _asserter) = mock_provider();
        let temp_dir = tempdir().unwrap();
        let config_path =
            create_dummy_config_file(&temp_dir, "actions.yaml", create_test_action_config_str());
        let network_id = "testnet";

        let action =
            ActionBuilder::new("Dummy Action").webhook_config("http://example.com").build();

        let mut mock_repo = MockAppRepository::new();
        // Expect get_actions to be called and return non-empty
        mock_repo
            .expect_get_actions()
            .with(eq(network_id))
            .once()
            .returning(move |_| Ok(vec![action.clone()]));
        // Expect clear_actions and add_actions to NOT be called
        mock_repo.expect_clear_actions().times(0);
        mock_repo.expect_add_actions().times(0);

        // Dummy config for AppConfig
        let config = AppConfig::builder()
            .network_id(network_id)
            .action_config_path(config_path.to_str().unwrap())
            .build();

        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Dummy path, won't be used
        let script_validator = create_script_validator();
        let initialization_service = InitializationService::new(
            config,
            Arc::new(mock_repo),
            abi_service,
            script_validator,
            provider,
        );

        let result = initialization_service.load_actions_from_file().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_load_abis_from_monitors() {
        let (provider, _asserter) = mock_provider();
        let temp_dir = tempdir().unwrap();
        let network_id = "testnet";
        let monitor = MonitorBuilder::new()
            .name("ABI Monitor")
            .network(network_id)
            .address("0x0000000000000000000000000000000000000001")
            .abi("test_abi")
            .build();

        let mut mock_repo = MockAppRepository::new();
        // Expect get_monitors to return a monitor with an ABI path
        mock_repo
            .expect_get_monitors()
            .with(eq(network_id))
            .once()
            .returning(move |_| Ok(vec![monitor.clone()]));

        let (abi_service, _) = create_test_abi_service(
            &temp_dir,
            &[("test_abi", r#"[{"type":"function","name":"testFunc","inputs":[],"outputs":[]}]"#)],
        );
        let initial_abi_cache_size = abi_service.cache_size();

        // Dummy config for AppConfig
        let config = AppConfig::builder().network_id(network_id).build();
        let script_validator = create_script_validator();
        let initialization_service = InitializationService::new(
            config,
            Arc::new(mock_repo),
            Arc::clone(&abi_service),
            script_validator,
            provider,
        );

        let result = initialization_service.load_abis_from_monitors().await;
        assert!(result.is_ok());

        // Verify ABI was added to AbiService
        assert_eq!(abi_service.cache_size(), initial_abi_cache_size + 1);
        assert!(abi_service.is_monitored(&address!("0x0000000000000000000000000000000000000001")));
    }

    #[tokio::test]
    async fn test_load_abis_from_monitors_global_abi() {
        let (provider, _asserter) = mock_provider();
        let temp_dir = tempdir().unwrap();
        let network_id = "testnet";
        let monitor = MonitorBuilder::new()
            .name("Global ABI Monitor")
            .network(network_id)
            .address("all") // This is the key for global ABI
            .abi("global_test_abi")
            .build();

        let mut mock_repo = MockAppRepository::new();
        mock_repo
            .expect_get_monitors()
            .with(eq(network_id))
            .once()
            .returning(move |_| Ok(vec![monitor.clone()]));

        let (abi_service, _) = create_test_abi_service(
            &temp_dir,
            &[(
                "global_test_abi",
                r#"[{"type":"event","name":"GlobalEvent","inputs":[],"anonymous":false}]"#,
            )],
        );
        let config = AppConfig::builder().network_id(network_id).build();
        let script_validator = create_script_validator();
        let initialization_service = InitializationService::new(
            config,
            Arc::new(mock_repo),
            Arc::clone(&abi_service),
            script_validator,
            provider,
        );

        let result = initialization_service.load_abis_from_monitors().await;
        assert!(result.is_ok());

        // Verify ABI was added to AbiService's global cache
        assert!(abi_service.has_global_abi("global_test_abi"));
    }

    #[tokio::test]
    async fn test_initialize_block_state_when_db_not_empty_does_nothing() {
        let (provider, _asserter) = mock_provider();
        let network_id = "testnet";

        let mut mock_repo = MockAppRepository::new();
        // Simulate existing block state
        mock_repo
            .expect_get_last_processed_block()
            .with(eq(network_id))
            .once()
            .returning(|_| Ok(Some(100)));
        // Ensure set_last_processed_block is NOT called
        mock_repo.expect_set_last_processed_block().times(0);

        let config = AppConfig::builder().network_id(network_id).build();
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Dummy path, won't be used
        let script_validator = create_script_validator();
        let initialization_service = InitializationService::new(
            config,
            Arc::new(mock_repo),
            abi_service,
            script_validator,
            provider,
        );

        let result = initialization_service.initialize_block_state().await;
        assert!(
            result.is_ok(),
            "Expected initialization to succeed, but got error: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_initialize_block_state_with_absolute_start_block() {
        let (provider, asserter) = mock_provider();
        let network_id = "testnet";

        let mut mock_repo = MockAppRepository::new();
        // Simulate no existing block state
        mock_repo
            .expect_get_last_processed_block()
            .with(eq(network_id))
            .once()
            .returning(|_| Ok(None));
        // Expect set_last_processed_block to be called with start_block - 1
        mock_repo
            .expect_set_last_processed_block()
            .with(eq(network_id), eq(49)) // 50 - 1
            .once()
            .returning(|_, _| Ok(()));

        // Mock provider to return latest block number
        asserter.push_success(&U256::from(100));

        let config = AppConfig::builder()
            .network_id(network_id)
            .initial_start_block(InitialStartBlock::Absolute(50))
            .confirmation_blocks(5)
            .build();

        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Dummy path, won't be used
        let script_validator = create_script_validator();
        let initialization_service = InitializationService::new(
            config,
            Arc::new(mock_repo),
            abi_service,
            script_validator,
            provider,
        );

        let result = initialization_service.initialize_block_state().await;
        assert!(
            result.is_ok(),
            "Expected initialization to succeed, but got error: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_initialize_block_state_with_latest_start_block() {
        let (provider, asserter) = mock_provider();
        let network_id = "testnet";

        let mut mock_repo = MockAppRepository::new();
        // Simulate no existing block state
        mock_repo
            .expect_get_last_processed_block()
            .with(eq(network_id))
            .once()
            .returning(|_| Ok(None));
        // Expect set_last_processed_block to be called with start_block - 1
        mock_repo
            .expect_set_last_processed_block()
            .with(eq(network_id), eq(94)) // 100 - 5 (confirmation blocks) - 1
            .once()
            .returning(|_, _| Ok(()));

        // Mock provider to return latest block number
        asserter.push_success(&U256::from(100));

        let config = AppConfig::builder()
            .network_id(network_id)
            .initial_start_block(InitialStartBlock::Latest)
            .confirmation_blocks(5)
            .build();

        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Dummy path, won't be used
        let script_validator = create_script_validator();
        let initialization_service = InitializationService::new(
            config,
            Arc::new(mock_repo),
            abi_service,
            script_validator,
            provider,
        );

        let result = initialization_service.initialize_block_state().await;
        assert!(
            result.is_ok(),
            "Expected initialization to succeed, but got error: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_initialize_block_state_clamps_to_safe_head() {
        let (provider, asserter) = mock_provider();
        let network_id = "testnet";

        let mut mock_repo = MockAppRepository::new();
        // Simulate no existing block state
        mock_repo
            .expect_get_last_processed_block()
            .with(eq(network_id))
            .once()
            .returning(|_| Ok(None));
        // Expect set_last_processed_block to be called with safe head - 1
        mock_repo
            .expect_set_last_processed_block()
            .with(eq(network_id), eq(94)) // 100 - 5 (confirmation blocks) - 1
            .once()
            .returning(|_, _| Ok(()));

        // Mock provider to return latest block number
        asserter.push_success(&U256::from(100));

        let config = AppConfig::builder()
            .network_id(network_id)
            .initial_start_block(InitialStartBlock::Absolute(150)) // Beyond latest block
            .confirmation_blocks(5)
            .build();

        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Dummy path, won't be used
        let script_validator = create_script_validator();
        let initialization_service = InitializationService::new(
            config,
            Arc::new(mock_repo),
            abi_service,
            script_validator,
            provider,
        );

        let result = initialization_service.initialize_block_state().await;
        assert!(
            result.is_ok(),
            "Expected initialization to succeed, but got error: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_initialize_block_state_handles_rpc_failure() {
        let (provider, _asserter) = mock_provider();
        let network_id = "testnet";

        let mut mock_repo = MockAppRepository::new();
        // Simulate no existing block state
        mock_repo
            .expect_get_last_processed_block()
            .with(eq(network_id))
            .once()
            .returning(|_| Ok(None));
        // Ensure set_last_processed_block is NOT called
        mock_repo.expect_set_last_processed_block().times(0);

        // Mock provider to simulate RPC failure
        // No success pushed to asserter, so it will return error
        let config = AppConfig::builder()
            .network_id(network_id)
            .initial_start_block(InitialStartBlock::Latest)
            .confirmation_blocks(5)
            .build();

        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Dummy path, won't be used
        let script_validator = create_script_validator();
        let initialization_service = InitializationService::new(
            config,
            Arc::new(mock_repo),
            abi_service,
            script_validator,
            provider,
        );

        let result = initialization_service.initialize_block_state().await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            InitializationError::BlockStateInitialization(msg) if msg.contains("Failed to fetch latest block number")
        ));
    }

    #[tokio::test]
    async fn test_initialize_block_state_handles_repo_write_failure() {
        let (provider, asserter) = mock_provider();
        let network_id = "testnet";

        let mut mock_repo = MockAppRepository::new();
        // Simulate no existing block state
        mock_repo
            .expect_get_last_processed_block()
            .with(eq(network_id))
            .once()
            .returning(|_| Ok(None));
        // Simulate failure when setting last processed block
        mock_repo
            .expect_set_last_processed_block()
            .with(eq(network_id), eq(94)) // 100 - 5 (confirmation blocks) - 1
            .once()
            .returning(|_, _| Err(PersistenceError::OperationFailed("Write failed".to_string())));

        // Mock provider to return latest block number
        asserter.push_success(&U256::from(100));

        let config = AppConfig::builder()
            .network_id(network_id)
            .initial_start_block(InitialStartBlock::Latest)
            .confirmation_blocks(5)
            .build();

        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Dummy path, won't be used
        let script_validator = create_script_validator();
        let initialization_service = InitializationService::new(
            config,
            Arc::new(mock_repo),
            abi_service,
            script_validator,
            provider,
        );

        let result = initialization_service.initialize_block_state().await;
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            matches!(
                error,
                InitializationError::BlockStateInitialization(ref msg) if msg.contains("Failed to set latest processed block")
            ),
            "Unexpected error message: {}",
            error
        );
    }
}
