//! This module provides the `InitializationService` responsible for loading
//! initial application data (monitors, triggers, ABIs) into the database and
//! ABI service at startup.

use std::{path::PathBuf, sync::Arc};

use thiserror::Error;

use crate::{
    abi::{AbiService, loader::AbiLoader},
    config::{AppConfig, TriggerLoader},
    monitor::{MonitorLoader, MonitorValidator},
    persistence::traits::StateRepository,
};

/// Errors that can occur during initialization.
#[derive(Debug, Error)]
pub enum InitializationError {
    /// An error occurred while loading monitors from the configuration file.
    #[error("Failed to load monitors from file: {0}")]
    MonitorLoadError(String),

    /// An error occurred while loading triggers from the configuration file.
    #[error("Failed to load triggers from file: {0}")]
    TriggerLoadError(String),

    /// An error occurred while loading ABIs from monitors.
    #[error("Failed to load ABIs from monitors: {0}")]
    AbiLoadError(String),
}

/// A service responsible for initializing application state at startup.
pub struct InitializationService {
    config: AppConfig,
    repo: Arc<dyn StateRepository>,
    abi_service: Arc<AbiService>,
}

impl InitializationService {
    /// Creates a new `InitializationService`.
    pub fn new(
        config: AppConfig,
        repo: Arc<dyn StateRepository>,
        abi_service: Arc<AbiService>,
    ) -> Self {
        Self { config, repo, abi_service }
    }

    /// Runs the initialization process, loading monitors, triggers, and ABIs.
    pub async fn run(&self) -> Result<(), InitializationError> {
        // Load monitors from the configuration file if specified and DB is empty.
        self.load_monitors_from_file().await?;

        // Load triggers from the configuration file if specified and DB is empty.
        self.load_triggers_from_file().await?;

        // Load ABIs into the AbiService from the monitors stored in the database.
        self.load_abis_from_monitors().await?;

        Ok(())
    }

    pub(crate) async fn load_monitors_from_file(&self) -> Result<(), InitializationError> {
        let network_id = &self.config.network_id;
        let config_path = &self.config.monitor_config_path;

        tracing::debug!(network_id = %network_id, "Checking for existing monitors in database...");
        let existing_monitors = self.repo.get_monitors(network_id).await.map_err(|e| {
            InitializationError::MonitorLoadError(format!(
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

        tracing::info!(config_path = %config_path, "No monitors found in database. Loading from configuration file...");
        let monitor_loader = MonitorLoader::new(PathBuf::from(config_path));
        let monitors = monitor_loader.load().map_err(|e| {
            InitializationError::MonitorLoadError(format!("Failed to load monitors from file: {e}"))
        })?;

        // Validate monitors
        let validator = MonitorValidator::new(network_id);
        for monitor in &monitors {
            validator.validate(monitor).map_err(|e| {
                InitializationError::MonitorLoadError(format!(
                    "Monitor validation failed for '{}': {}",
                    monitor.name, e
                ))
            })?;
        }

        let count = monitors.len();
        tracing::info!(count = count, "Loaded monitors from configuration file.");
        self.repo.clear_monitors(network_id).await.map_err(|e| {
            InitializationError::MonitorLoadError(format!(
                "Failed to clear existing monitors in DB: {e}"
            ))
        })?;
        self.repo.add_monitors(network_id, monitors).await.map_err(|e| {
            InitializationError::MonitorLoadError(format!("Failed to add monitors to DB: {e}"))
        })?;
        tracing::info!(count = count, network_id = %network_id, "Monitors from file stored in database.");
        Ok(())
    }

    pub(crate) async fn load_triggers_from_file(&self) -> Result<(), InitializationError> {
        let network_id = &self.config.network_id;
        let config_path = &self.config.trigger_config_path;

        tracing::debug!(network_id = %network_id, "Checking for existing triggers in database...");
        let existing_triggers = self.repo.get_triggers(network_id).await.map_err(|e| {
            InitializationError::TriggerLoadError(format!(
                "Failed to fetch existing triggers from DB: {e}"
            ))
        })?;

        if !existing_triggers.is_empty() {
            tracing::info!(
                network_id = %network_id,
                count = existing_triggers.len(),
                "Triggers already exist in the database. Skipping loading from file."
            );
            return Ok(());
        }

        tracing::info!(config_path = %config_path, "No triggers found in database. Loading from configuration file...");
        let trigger_loader = TriggerLoader::new(PathBuf::from(config_path));
        let triggers = trigger_loader.load().map_err(|e| {
            InitializationError::TriggerLoadError(format!("Failed to load triggers from file: {e}"))
        })?;
        let count = triggers.len();
        tracing::info!(count = count, "Loaded triggers from configuration file.");
        self.repo.clear_triggers(network_id).await.map_err(|e| {
            InitializationError::TriggerLoadError(format!(
                "Failed to clear existing triggers in DB: {e}"
            ))
        })?;
        self.repo.add_triggers(network_id, triggers).await.map_err(|e| {
            InitializationError::TriggerLoadError(format!("Failed to add triggers to DB: {e}"))
        })?;
        tracing::info!(count = count, network_id = %network_id, "Triggers from file stored in database.");
        Ok(())
    }

    pub(crate) async fn load_abis_from_monitors(&self) -> Result<(), InitializationError> {
        let network_id = &self.config.network_id;
        tracing::debug!(network_id = %network_id, "Loading ABIs from monitors in database...");
        let monitors = self.repo.get_monitors(network_id).await.map_err(|e| {
            InitializationError::AbiLoadError(format!(
                "Failed to fetch monitors for ABI loading: {e}"
            ))
        })?;

        for monitor in &monitors {
            if let (Some(address_str), Some(abi_path_str)) = (&monitor.address, &monitor.abi) {
                let address = address_str.parse().map_err(|e| {
                    InitializationError::AbiLoadError(format!(
                        "Failed to parse address for monitor '{}': {}",
                        monitor.name, e
                    ))
                })?;

                let abi_loader = AbiLoader::new(PathBuf::from(abi_path_str));
                let abi = abi_loader.load().map_err(|e| {
                    InitializationError::AbiLoadError(format!(
                        "Failed to load ABI for monitor '{}': {}",
                        monitor.name, e
                    ))
                })?;

                self.abi_service.add_abi(address, &abi);
            }
        }
        tracing::info!(count = monitors.len(), network_id = %network_id, "ABIs loaded for monitors from database.");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write};

    use mockall::predicate::*;
    use tempfile::tempdir;

    use super::*;
    use crate::{
        config::{AppConfig, HttpRetryConfig},
        models::{
            monitor::Monitor,
            notification::NotificationMessage,
            trigger::{SlackConfig, TriggerConfig, TriggerTypeConfig},
        },
        persistence::traits::MockStateRepository,
    };

    // Helper to create a dummy config file
    fn create_dummy_config_file(dir: &tempfile::TempDir, filename: &str, content: &str) -> PathBuf {
        let file_path = dir.path().join(filename);
        fs::write(&file_path, content).unwrap();
        file_path
    }

    fn create_test_abi_file(dir: &tempfile::TempDir, filename: &str, content: &str) -> PathBuf {
        let file_path = dir.path().join(filename);
        let mut file = fs::File::create(&file_path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file_path
    }

    fn create_test_trigger_config_str() -> &'static str {
        r#"
triggers:
  - name: "Test Trigger"
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
    abi: "abi.json"
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

    #[tokio::test]
    async fn test_run_happy_path_db_empty() {
        let temp_dir = tempdir().unwrap();
        let _ = create_test_abi_file(&temp_dir, "abi.json", create_test_abi_content());
        let monitor_config_path =
            create_dummy_config_file(&temp_dir, "monitors.yaml", create_test_monitor_config_str());
        let trigger_config_path =
            create_dummy_config_file(&temp_dir, "triggers.yaml", create_test_trigger_config_str());
        let network_id = "testnet";

        let mut mock_repo = MockStateRepository::new();
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
        // Triggers
        mock_repo.expect_get_triggers().with(eq(network_id)).once().returning(|_| Ok(vec![]));
        mock_repo.expect_clear_triggers().with(eq(network_id)).once().returning(|_| Ok(()));
        mock_repo
            .expect_add_triggers()
            .with(eq(network_id), always())
            .once()
            .returning(|_, _| Ok(()));

        let config = AppConfig::builder()
            .network_id(network_id)
            .monitor_config_path(monitor_config_path.to_str().unwrap())
            .trigger_config_path(trigger_config_path.to_str().unwrap())
            .build();

        let abi_service = Arc::new(AbiService::new());
        let initialization_service =
            InitializationService::new(config, Arc::new(mock_repo), abi_service);

        let result = initialization_service.run().await;

        println!("Initialization result: {:?}", result);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_run_skips_loading_if_db_not_empty() {
        let network_id = "testnet";
        let monitor = Monitor::from_config(
            "Existing Monitor".to_string(),
            network_id.to_string(),
            None,
            None,
            "true".to_string(),
        );
        let trigger = TriggerConfig {
            name: "Existing Trigger".to_string(),
            config: TriggerTypeConfig::Webhook(Default::default()),
        };

        let mut mock_repo = MockStateRepository::new();
        // Return existing monitors
        mock_repo
            .expect_get_monitors()
            .with(eq(network_id))
            .times(2) // Called once for monitor loading, once for ABI loading
            .returning(move |_| Ok(vec![monitor.clone()]));
        // Return existing triggers
        mock_repo
            .expect_get_triggers()
            .with(eq(network_id))
            .once()
            .returning(move |_| Ok(vec![trigger.clone()]));

        // Ensure file loading is NOT called
        mock_repo.expect_add_monitors().times(0);
        mock_repo.expect_add_triggers().times(0);

        let config = AppConfig::builder().network_id(network_id).build();
        let abi_service = Arc::new(AbiService::new());
        let initialization_service =
            InitializationService::new(config, Arc::new(mock_repo), abi_service);

        let result = initialization_service.run().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_load_monitors_from_file_repo_error() {
        let temp_dir = tempdir().unwrap();
        let config_path = create_dummy_config_file(&temp_dir, "monitors.yaml", "monitors: []");
        let network_id = "testnet";

        let mut mock_repo = MockStateRepository::new();
        mock_repo
            .expect_get_monitors()
            .with(eq(network_id))
            .once()
            .returning(|_| Err(sqlx::Error::RowNotFound)); // Simulate a DB error

        let config = AppConfig::builder()
            .network_id(network_id)
            .monitor_config_path(config_path.to_str().unwrap())
            .build();

        let abi_service = Arc::new(AbiService::new());
        let initialization_service =
            InitializationService::new(config, Arc::new(mock_repo), abi_service);

        let result = initialization_service.load_monitors_from_file().await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            InitializationError::MonitorLoadError(msg) if msg.contains("Failed to fetch existing monitors")
        ));
    }

    #[tokio::test]
    async fn test_load_abis_from_monitors_invalid_address() {
        let network_id = "testnet";
        let monitor = Monitor::from_config(
            "ABI Monitor".to_string(),
            network_id.to_string(),
            Some("not-a-valid-address".to_string()),
            Some("abi.json".to_string()),
            "true".to_string(),
        );

        let mut mock_repo = MockStateRepository::new();
        mock_repo
            .expect_get_monitors()
            .with(eq(network_id))
            .once()
            .returning(move |_| Ok(vec![monitor.clone()]));

        let config = AppConfig::builder().network_id(network_id).build();
        let abi_service = Arc::new(AbiService::new());
        let initialization_service =
            InitializationService::new(config, Arc::new(mock_repo), abi_service);

        let result = initialization_service.load_abis_from_monitors().await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            InitializationError::AbiLoadError(msg) if msg.contains("Failed to parse address")
        ));
    }

    #[tokio::test]
    async fn test_load_abis_from_monitors_abi_load_error() {
        let temp_dir = tempdir().unwrap();
        let non_existent_abi_path = temp_dir.path().join("non_existent_abi.json");
        let network_id = "testnet";
        let monitor = Monitor::from_config(
            "ABI Monitor".to_string(),
            network_id.to_string(),
            Some("0x0000000000000000000000000000000000000001".to_string()),
            Some(non_existent_abi_path.to_str().unwrap().to_string()),
            "true".to_string(),
        );

        let mut mock_repo = MockStateRepository::new();
        mock_repo
            .expect_get_monitors()
            .with(eq(network_id))
            .once()
            .returning(move |_| Ok(vec![monitor.clone()]));

        let config = AppConfig::builder().network_id(network_id).build();
        let abi_service = Arc::new(AbiService::new());
        let initialization_service =
            InitializationService::new(config, Arc::new(mock_repo), abi_service);

        let result = initialization_service.load_abis_from_monitors().await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            InitializationError::AbiLoadError(msg) if msg.contains("Failed to load ABI")
        ));
    }

    #[tokio::test]
    async fn test_load_monitors_from_file_when_db_empty() {
        let temp_dir = tempdir().unwrap();
        let _ = create_test_abi_file(&temp_dir, "abi.json", create_test_abi_content());
        let config_path =
            create_dummy_config_file(&temp_dir, "monitors.yaml", create_test_monitor_config_str());
        let network_id = "testnet";

        let mut mock_repo = MockStateRepository::new();
        // Expect get_monitors to be called and return empty
        mock_repo.expect_get_monitors().with(eq(network_id)).once().returning(|_| Ok(vec![]));
        // Expect clear_monitors and add_monitors to be called
        mock_repo.expect_clear_monitors().with(eq(network_id)).once().returning(|_| Ok(()));
        mock_repo
            .expect_add_monitors()
            .with(eq(network_id), function(|monitors: &Vec<Monitor>| monitors.len() == 1))
            .once()
            .returning(|_, _| Ok(()));

        // Dummy config for AppConfig
        let config = AppConfig::builder()
            .network_id(network_id)
            .monitor_config_path(config_path.to_str().unwrap())
            .build();

        let abi_service = Arc::new(AbiService::new());
        let initialization_service =
            InitializationService::new(config, Arc::new(mock_repo), abi_service);

        let result = initialization_service.load_monitors_from_file().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_load_monitors_from_file_when_db_empty_invalid_monitor() {
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

        let mut mock_repo = MockStateRepository::new();
        // Expect get_monitors to be called and return empty
        mock_repo.expect_get_monitors().with(eq(network_id)).once().returning(|_| Ok(vec![]));

        // Dummy config for AppConfig
        let config = AppConfig::builder()
            .network_id(network_id)
            .monitor_config_path(config_path.to_str().unwrap())
            .build();

        let abi_service = Arc::new(AbiService::new());
        let initialization_service =
            InitializationService::new(config, Arc::new(mock_repo), abi_service);

        let result = initialization_service.load_monitors_from_file().await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            InitializationError::MonitorLoadError(msg) if msg.contains("Monitor validation failed")
        ));
    }

    #[tokio::test]
    async fn test_load_monitors_from_file_when_db_not_empty() {
        let temp_dir = tempdir().unwrap();
        let config_path =
            create_dummy_config_file(&temp_dir, "monitors.yaml", create_test_monitor_config_str());
        let monitor = Monitor::from_config(
            "Dummy Monitor".to_string(),
            "testnet".to_string(),
            None,
            None,
            "true".to_string(),
        );
        let network_id = "testnet";

        let mut mock_repo = MockStateRepository::new();
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

        let abi_service = Arc::new(AbiService::new());
        let initialization_service =
            InitializationService::new(config, Arc::new(mock_repo), abi_service);

        let result = initialization_service.load_monitors_from_file().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_load_triggers_from_file_when_db_empty() {
        let temp_dir = tempdir().unwrap();
        let config_path =
            create_dummy_config_file(&temp_dir, "triggers.yaml", create_test_trigger_config_str());
        let network_id = "testnet";

        let mut mock_repo = MockStateRepository::new();
        // Expect get_triggers to be called and return empty
        mock_repo.expect_get_triggers().with(eq(network_id)).once().returning(|_| Ok(vec![]));
        // Expect clear_triggers and add_triggers to be called
        mock_repo.expect_clear_triggers().with(eq(network_id)).once().returning(|_| Ok(()));
        mock_repo
            .expect_add_triggers()
            .with(eq(network_id), function(|triggers: &Vec<TriggerConfig>| triggers.len() == 1))
            .once()
            .returning(|_, _| Ok(()));

        // Dummy config for AppConfig
        let config = AppConfig::builder()
            .network_id(network_id)
            .trigger_config_path(config_path.to_str().unwrap())
            .build();

        let abi_service = Arc::new(AbiService::new());
        let initialization_service =
            InitializationService::new(config, Arc::new(mock_repo), abi_service);

        let result = initialization_service.load_triggers_from_file().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_load_triggers_from_file_when_db_not_empty() {
        let temp_dir = tempdir().unwrap();
        let config_path =
            create_dummy_config_file(&temp_dir, "triggers.yaml", create_test_trigger_config_str());
        let network_id = "testnet";

        let mut mock_repo = MockStateRepository::new();
        // Expect get_triggers to be called and return non-empty
        mock_repo.expect_get_triggers().with(eq(network_id)).once().returning(|_| {
            Ok(vec![TriggerConfig {
                name: "Dummy Trigger".to_string(),
                config: TriggerTypeConfig::Slack(SlackConfig {
                    slack_url: "http://dummy.url".to_string(),
                    message: NotificationMessage::default(),
                    retry_policy: HttpRetryConfig::default(),
                }),
            }])
        }); // Return a dummy trigger
        // Expect clear_triggers and add_triggers to NOT be called
        mock_repo.expect_clear_triggers().times(0);
        mock_repo.expect_add_triggers().times(0);

        // Dummy config for AppConfig
        let config = AppConfig::builder()
            .network_id(network_id)
            .trigger_config_path(config_path.to_str().unwrap())
            .build();

        let abi_service = Arc::new(AbiService::new());
        let initialization_service =
            InitializationService::new(config, Arc::new(mock_repo), abi_service);

        let result = initialization_service.load_triggers_from_file().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_load_abis_from_monitors() {
        let temp_dir = tempdir().unwrap();
        let abi_path = create_dummy_config_file(
            &temp_dir,
            "test_abi.json",
            r#"[{"type":"function","name":"testFunc","inputs":[],"outputs":[]}]"#,
        );
        let network_id = "testnet";
        let monitor = Monitor::from_config(
            "ABI Monitor".to_string(),
            network_id.to_string(),
            Some("0x0000000000000000000000000000000000000001".to_string()),
            Some(abi_path.to_str().unwrap().to_string()),
            "true".to_string(),
        );

        let mut mock_repo = MockStateRepository::new();
        // Expect get_monitors to return a monitor with an ABI path
        mock_repo
            .expect_get_monitors()
            .with(eq(network_id))
            .once()
            .returning(move |_| Ok(vec![monitor.clone()]));

        let abi_service = Arc::new(AbiService::new());
        let initial_abi_cache_size = abi_service.cache_size();

        // Dummy config for AppConfig
        let config = AppConfig::builder().network_id(network_id).build();

        let initialization_service =
            InitializationService::new(config, Arc::new(mock_repo), Arc::clone(&abi_service));

        let result = initialization_service.load_abis_from_monitors().await;
        assert!(result.is_ok());

        // Verify ABI was added to AbiService
        assert_eq!(abi_service.cache_size(), initial_abi_cache_size + 1);
        assert!(
            abi_service
                .is_monitored(&"0x0000000000000000000000000000000000000001".parse().unwrap())
        );
    }
}
