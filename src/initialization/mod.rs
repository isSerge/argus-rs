//! This module provides the `InitializationService` responsible for loading
//! initial application data (monitors, triggers, ABIs) into the database and
//! ABI service at startup.

use std::{path::PathBuf, sync::Arc};

use crate::{
    abi::{AbiService, loader::AbiLoader},
    config::{AppConfig, TriggerLoader},
    monitor::{MonitorLoader, MonitorValidator},
    persistence::traits::StateRepository,
};

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
        Self {
            config,
            repo,
            abi_service,
        }
    }

    /// Runs the initialization process, loading monitors, triggers, and ABIs.
    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Load monitors from the configuration file if specified and DB is empty.
        self.load_monitors_from_file().await?;

        // Load triggers from the configuration file if specified and DB is empty.
        self.load_triggers_from_file().await?;

        // Load ABIs into the AbiService from the monitors stored in the database.
        self.load_abis_from_monitors().await?;

        Ok(())
    }

    pub(crate) async fn load_monitors_from_file(&self) -> Result<(), Box<dyn std::error::Error>> {
        let network_id = &self.config.network_id;
        let config_path = &self.config.monitor_config_path;

        tracing::debug!(network_id = %network_id, "Checking for existing monitors in database...");
        let existing_monitors = self.repo.get_monitors(network_id).await?;

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
        let monitors = monitor_loader.load()?;

        // Validate monitors
        let validator = MonitorValidator::new(network_id);
        for monitor in &monitors {
            validator.validate(monitor)?;
        }

        let count = monitors.len();
        tracing::info!(count = count, "Loaded monitors from configuration file.");
        self.repo.clear_monitors(network_id).await?;
        self.repo.add_monitors(network_id, monitors).await?;
        tracing::info!(count = count, network_id = %network_id, "Monitors from file stored in database.");
        Ok(())
    }

    pub(crate) async fn load_triggers_from_file(&self) -> Result<(), Box<dyn std::error::Error>> {
        let network_id = &self.config.network_id;
        let config_path = &self.config.trigger_config_path;

        tracing::debug!(network_id = %network_id, "Checking for existing triggers in database...");
        let existing_triggers = self.repo.get_triggers(network_id).await?;

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
        let triggers = trigger_loader.load()?;
        let count = triggers.len();
        tracing::info!(count = count, "Loaded triggers from configuration file.");
        self.repo.clear_triggers(network_id).await?;
        self.repo.add_triggers(network_id, triggers).await?;
        tracing::info!(count = count, network_id = %network_id, "Triggers from file stored in database.");
        Ok(())
    }

    pub(crate) async fn load_abis_from_monitors(&self) -> Result<(), Box<dyn std::error::Error>> {
        let network_id = &self.config.network_id;
        tracing::debug!(network_id = %network_id, "Loading ABIs from monitors in database...");
        let monitors = self.repo.get_monitors(network_id).await?;

        for monitor in &monitors {
            if let (Some(address_str), Some(abi_path_str)) = (&monitor.address, &monitor.abi) {
                let address = address_str.parse().map_err(|_| {
                    format!(
                        "Invalid address '{}' for monitor '{}'",
                        address_str, monitor.name
                    )
                })?;

                let abi_loader = AbiLoader::new(PathBuf::from(abi_path_str));
                let abi = abi_loader.load().map_err(|e| {
                    format!("Failed to load ABI for monitor '{}': {}", monitor.name, e)
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
    use super::*;
    use crate::config::{AppConfig, HttpRetryConfig};
    use crate::models::notification::NotificationMessage;
    use crate::models::{
        monitor::Monitor,
        trigger::{SlackConfig, TriggerConfig, TriggerTypeConfig},
    };
    use crate::persistence::traits::MockStateRepository;
    use mockall::predicate::*;
    use std::fs;
    use tempfile::tempdir;

    // Helper to create a dummy config file
    fn create_dummy_config_file(dir: &tempfile::TempDir, filename: &str, content: &str) -> PathBuf {
        let file_path = dir.path().join(filename);
        fs::write(&file_path, content).unwrap();
        file_path
    }

    #[tokio::test]
    async fn test_load_monitors_from_file_when_db_empty() {
        let temp_dir = tempdir().unwrap();
        let config_path = create_dummy_config_file(
            &temp_dir,
            "monitors.yaml",
            r#"
monitors:
  - name: "Test Monitor"
    network: "testnet"
    address: "0x0000000000000000000000000000000000000001"
    filter_script: "true"
"#,
        );
        let network_id = "testnet";

        let mut mock_repo = MockStateRepository::new();
        // Expect get_monitors to be called and return empty
        mock_repo
            .expect_get_monitors()
            .with(eq(network_id))
            .once()
            .returning(|_| Ok(vec![]));
        // Expect clear_monitors and add_monitors to be called
        mock_repo
            .expect_clear_monitors()
            .with(eq(network_id))
            .once()
            .returning(|_| Ok(()));
        mock_repo
            .expect_add_monitors()
            .with(
                eq(network_id),
                function(|monitors: &Vec<Monitor>| monitors.len() == 1),
            )
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
        mock_repo
            .expect_get_monitors()
            .with(eq(network_id))
            .once()
            .returning(|_| Ok(vec![]));

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
        let err = result.unwrap_err();
        let err_str = format!("{}", err);
        assert!(err_str.contains("accesses log data but does not have an ABI defined"));
    }

    #[tokio::test]
    async fn test_load_monitors_from_file_when_db_not_empty() {
        let temp_dir = tempdir().unwrap();
        let config_path = create_dummy_config_file(
            &temp_dir,
            "monitors.yaml",
            r#"
monitors:
  - name: "Test Monitor"
    network: "testnet"
    address: "0x0000000000000000000000000000000000000001"
    filter_script: "true"
"#,
        );
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
        let config_path = create_dummy_config_file(
            &temp_dir,
            "triggers.yaml",
            r#"
triggers:
  - name: "Test Trigger"
    webhook:
      url: "http://example.com/webhook"
      message:
        title: "Test Title"
        body: "Test Body"
"#,
        );
        let network_id = "testnet";

        let mut mock_repo = MockStateRepository::new();
        // Expect get_triggers to be called and return empty
        mock_repo
            .expect_get_triggers()
            .with(eq(network_id))
            .once()
            .returning(|_| Ok(vec![]));
        // Expect clear_triggers and add_triggers to be called
        mock_repo
            .expect_clear_triggers()
            .with(eq(network_id))
            .once()
            .returning(|_| Ok(()));
        mock_repo
            .expect_add_triggers()
            .with(
                eq(network_id),
                function(|triggers: &Vec<TriggerConfig>| triggers.len() == 1),
            )
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
        let config_path = create_dummy_config_file(
            &temp_dir,
            "triggers.yaml",
            r#"
triggers:
  - name: "Test Trigger"
    webhook:
      url: "http://example.com/webhook"
      message:
        title: "Test Title"
        body: "Test Body"
"#,
        );
        let network_id = "testnet";

        let mut mock_repo = MockStateRepository::new();
        // Expect get_triggers to be called and return non-empty
        mock_repo
            .expect_get_triggers()
            .with(eq(network_id))
            .once()
            .returning(|_| {
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
            abi_service.is_monitored(
                &"0x0000000000000000000000000000000000000001"
                    .parse()
                    .unwrap()
            )
        );
    }
}
