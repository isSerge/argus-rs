//! This module provides the `InitializationService` responsible for loading
//! initial application data (monitors, notifiers, ABIs) into the database and
//! ABI service at startup.

use std::{path::PathBuf, sync::Arc};

use thiserror::Error;

use crate::{
    abi::AbiService,
    config::AppConfig,
    engine::rhai::RhaiScriptValidator,
    loader::load_config,
    models::{monitor::MonitorConfig, notifier::NotifierConfig},
    monitor::MonitorValidator,
    persistence::traits::StateRepository,
};

// TODO: wrap the specific underlying error instead of String after introducing
// custom error to replace sqlx::Error
/// Errors that can occur during initialization.
#[derive(Debug, Error)]
pub enum InitializationError {
    /// An error occurred while loading monitors from the configuration file.
    #[error("Failed to load monitors from file: {0}")]
    MonitorLoadError(String),

    /// An error occurred while loading notifier from the configuration file.
    #[error("Failed to load notifier from file: {0}")]
    NotifierLoadError(String),

    /// An error occurred while loading ABIs from monitors.
    #[error("Failed to load ABIs from monitors: {0}")]
    AbiLoadError(String),
}

/// A service responsible for initializing application state at startup.
pub struct InitializationService {
    config: AppConfig,
    repo: Arc<dyn StateRepository>,
    abi_service: Arc<AbiService>,
    script_validator: RhaiScriptValidator,
}

impl InitializationService {
    /// Creates a new `InitializationService`.
    pub fn new(
        config: AppConfig,
        repo: Arc<dyn StateRepository>,
        abi_service: Arc<AbiService>,
        script_validator: RhaiScriptValidator,
    ) -> Self {
        Self { config, repo, abi_service, script_validator }
    }

    /// Runs the initialization process, loading monitors, notifiers, and ABIs.
    pub async fn run(&self) -> Result<(), InitializationError> {
        // Load notifiers from the configuration file if specified and DB is empty.
        self.load_notifiers_from_file().await?;

        // Load monitors from the configuration file if specified and DB is empty.
        self.load_monitors_from_file().await?;

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

        tracing::info!(config_path = %config_path.display(), "No monitors found in database. Loading from configuration file...");
        let monitors = load_config::<MonitorConfig>(PathBuf::from(config_path)).map_err(|e| {
            InitializationError::MonitorLoadError(format!("Failed to load monitors from file: {e}"))
        })?;

        // Validate monitors
        let notifiers = self.repo.get_notifiers(network_id).await.map_err(|e| {
            InitializationError::MonitorLoadError(format!(
                "Failed to fetch notifiers for monitor validation: {e}"
            ))
        })?;

        let validator = MonitorValidator::new(
            self.script_validator.clone(),
            self.abi_service.clone(),
            network_id,
            &notifiers,
        );
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

    pub(crate) async fn load_notifiers_from_file(&self) -> Result<(), InitializationError> {
        let network_id = &self.config.network_id;
        let config_path = &self.config.notifier_config_path;

        tracing::debug!(network_id = %network_id, "Checking for existing notifiers in database...");
        let existing_notifiers = self.repo.get_notifiers(network_id).await.map_err(|e| {
            InitializationError::NotifierLoadError(format!(
                "Failed to fetch existing notifiers from DB: {e}"
            ))
        })?;

        if !existing_notifiers.is_empty() {
            tracing::info!(
                network_id = %network_id,
                count = existing_notifiers.len(),
                "Notifiers already exist in the database. Skipping loading from file."
            );
            return Ok(());
        }

        tracing::info!(config_path = %config_path.display(), "No notifiers found in database. Loading from configuration file...");
        let notifiers = load_config::<NotifierConfig>(PathBuf::from(config_path)).map_err(|e| {
            InitializationError::NotifierLoadError(format!(
                "Failed to load notifiers from file: {e}"
            ))
        })?;
        let count = notifiers.len();
        tracing::info!(count = count, "Loaded notifiers from configuration file.");
        self.repo.clear_notifiers(network_id).await.map_err(|e| {
            InitializationError::NotifierLoadError(format!(
                "Failed to clear existing notifiers in DB: {e}"
            ))
        })?;
        self.repo.add_notifiers(network_id, notifiers).await.map_err(|e| {
            InitializationError::NotifierLoadError(format!("Failed to add notifiers to DB: {e}"))
        })?;
        tracing::info!(count = count, network_id = %network_id, "Notifiers from file stored in database.");
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
            if let (Some(address_str), Some(abi_name)) = (&monitor.address, &monitor.abi) {
                if address_str.eq_ignore_ascii_case("all") {
                    self.abi_service.add_global_abi(abi_name).map_err(|e| {
                        InitializationError::AbiLoadError(format!(
                            "Failed to add global ABI '{}' for monitor '{}': {}",
                            abi_name, monitor.name, e
                        ))
                    })?;
                } else {
                    let address = address_str.parse().map_err(|e| {
                        InitializationError::AbiLoadError(format!(
                            "Failed to parse address for monitor '{}': {}",
                            monitor.name, e
                        ))
                    })?;

                    self.abi_service.link_abi(address, abi_name).map_err(|e| {
                        InitializationError::AbiLoadError(format!(
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

    use alloy::primitives::address;
    use mockall::predicate::*;
    use tempfile::tempdir;

    use super::*;
    use crate::{
        config::{AppConfig, HttpRetryConfig, RhaiConfig},
        engine::rhai::RhaiCompiler,
        models::{
            builder::MonitorBuilder,
            monitor::MonitorConfig,
            notification::NotificationMessage,
            notifier::{NotifierConfig, NotifierTypeConfig, SlackConfig},
        },
        persistence::traits::MockStateRepository,
        test_helpers::create_test_abi_service,
    };

    // Helper to create a dummy config file
    fn create_dummy_config_file(dir: &tempfile::TempDir, filename: &str, content: &str) -> PathBuf {
        let file_path = dir.path().join(filename);
        fs::write(&file_path, content).unwrap();
        file_path
    }

    fn create_test_notifier_config_str() -> &'static str {
        r#"
notifiers:
  - name: "Test Notifier"
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
        let temp_dir = tempdir().unwrap();
        let monitor_config_path =
            create_dummy_config_file(&temp_dir, "monitors.yaml", create_test_monitor_config_str());
        let notifier_config_path = create_dummy_config_file(
            &temp_dir,
            "notifiers.yaml",
            create_test_notifier_config_str(),
        );
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
        // Notifiers
        mock_repo
            .expect_get_notifiers()
            .with(eq(network_id))
            .times(2) // Called for notifier loading and monitor validation
            .returning(|_| Ok(vec![]));
        mock_repo.expect_clear_notifiers().with(eq(network_id)).once().returning(|_| Ok(()));
        mock_repo
            .expect_add_notifiers()
            .with(eq(network_id), always())
            .once()
            .returning(|_, _| Ok(()));

        let config = AppConfig::builder()
            .network_id(network_id)
            .monitor_config_path(monitor_config_path.to_str().unwrap())
            .notifier_config_path(notifier_config_path.to_str().unwrap())
            .abi_config_path(temp_dir.path().to_str().unwrap())
            .build();

        let (abi_service, _) =
            create_test_abi_service(&temp_dir, &[("test_abi", create_test_abi_content())]);
        // Link the ABI for the test monitor
        abi_service
            .link_abi("0x0000000000000000000000000000000000000123".parse().unwrap(), "test_abi")
            .unwrap();
        let script_validator = create_script_validator();
        let initialization_service =
            InitializationService::new(config, Arc::new(mock_repo), abi_service, script_validator);

        let result = initialization_service.run().await;

        println!("Initialization result: {:?}", result);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_run_skips_loading_if_db_not_empty() {
        let network_id = "testnet";

        let monitor = MonitorBuilder::new().network(network_id).name("Existing Monitor").build();

        let trigger = NotifierConfig {
            name: "Existing Trigger".to_string(),
            config: NotifierTypeConfig::Webhook(Default::default()),
            policy: None,
        };

        let mut mock_repo = MockStateRepository::new();
        // Return existing monitors
        mock_repo
            .expect_get_monitors()
            .with(eq(network_id))
            .times(2) // Called once for monitor loading, once for ABI loading
            .returning(move |_| Ok(vec![monitor.clone()]));
        // Return existing notifiers
        mock_repo
            .expect_get_notifiers()
            .with(eq(network_id))
            .once()
            .returning(move |_| Ok(vec![trigger.clone()]));

        // Ensure file loading is NOT called
        mock_repo.expect_add_monitors().times(0);
        mock_repo.expect_add_notifiers().times(0);

        let config = AppConfig::builder().network_id(network_id).build();
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Dummy path, won't be used
        let script_validator = create_script_validator();
        let initialization_service =
            InitializationService::new(config, Arc::new(mock_repo), abi_service, script_validator);

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

        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Dummy path, won't be used
        let script_validator = create_script_validator();
        let initialization_service =
            InitializationService::new(config, Arc::new(mock_repo), abi_service, script_validator);

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

        let monitor = MonitorBuilder::new()
            .network(network_id)
            .name("ABI Monitor")
            .address("not-a-valid-address")
            .abi("test_abi")
            .build();

        let mut mock_repo = MockStateRepository::new();
        mock_repo
            .expect_get_monitors()
            .with(eq(network_id))
            .once()
            .returning(move |_| Ok(vec![monitor.clone()]));

        let config = AppConfig::builder().network_id(network_id).build();
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Dummy path, won't be used
        let script_validator = create_script_validator();
        let initialization_service =
            InitializationService::new(config, Arc::new(mock_repo), abi_service, script_validator);

        let result = initialization_service.load_abis_from_monitors().await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            InitializationError::AbiLoadError(msg) if msg.contains("Failed to parse address")
        ));
    }

    #[tokio::test]
    async fn test_load_abis_from_monitors_abi_not_found_in_repo() {
        let temp_dir = tempdir().unwrap();
        // No ABI file created, so repository will be empty
        let network_id = "testnet";
        let monitor = MonitorBuilder::new()
            .name("ABI Monitor")
            .network(network_id)
            .address("0x0000000000000000000000000000000000000001")
            .abi("non_existent_abi")
            .build();

        let mut mock_repo = MockStateRepository::new();
        mock_repo
            .expect_get_monitors()
            .with(eq(network_id))
            .once()
            .returning(move |_| Ok(vec![monitor.clone()]));

        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Empty repo

        let config = AppConfig::builder().network_id(network_id).build();
        let script_validator = create_script_validator();
        let initialization_service =
            InitializationService::new(config, Arc::new(mock_repo), abi_service, script_validator);

        let result = initialization_service.load_abis_from_monitors().await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            InitializationError::AbiLoadError(msg) if msg.contains("Failed to link ABI 'non_existent_abi'")
        ));
    }

    #[tokio::test]
    async fn test_load_monitors_from_file_when_db_empty() {
        let temp_dir = tempdir().unwrap();
        let config_path =
            create_dummy_config_file(&temp_dir, "monitors.yaml", create_test_monitor_config_str());
        let network_id = "testnet";

        let mut mock_repo = MockStateRepository::new();
        // Expect get_monitors to be called and return empty
        mock_repo.expect_get_monitors().with(eq(network_id)).once().returning(|_| Ok(vec![]));
        // Expect get_notifiers to be called for validation
        mock_repo.expect_get_notifiers().with(eq(network_id)).once().returning(|_| Ok(vec![]));
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
        let initialization_service =
            InitializationService::new(config, Arc::new(mock_repo), abi_service, script_validator);

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
        // Expect get_notifiers to be called for validation
        mock_repo.expect_get_notifiers().with(eq(network_id)).once().returning(|_| Ok(vec![]));

        // Dummy config for AppConfig
        let config = AppConfig::builder()
            .network_id(network_id)
            .monitor_config_path(config_path.to_str().unwrap())
            .build();

        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Dummy path, won't be used
        let script_validator = create_script_validator();
        let initialization_service =
            InitializationService::new(config, Arc::new(mock_repo), abi_service, script_validator);

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
        let network_id = "testnet";
        let monitor = MonitorBuilder::new().network(network_id).name("Dummy Monitor").build();

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

        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Dummy path, won't be used
        let script_validator = create_script_validator();
        let initialization_service =
            InitializationService::new(config, Arc::new(mock_repo), abi_service, script_validator);

        let result = initialization_service.load_monitors_from_file().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_load_notifiers_from_file_when_db_empty() {
        let temp_dir = tempdir().unwrap();
        let config_path = create_dummy_config_file(
            &temp_dir,
            "notifiers.yaml",
            create_test_notifier_config_str(),
        );
        let network_id = "testnet";

        let mut mock_repo = MockStateRepository::new();
        // Expect get_notifiers to be called and return empty
        mock_repo.expect_get_notifiers().with(eq(network_id)).once().returning(|_| Ok(vec![]));
        // Expect clear_notifiers and add_notifiers to be called
        mock_repo.expect_clear_notifiers().with(eq(network_id)).once().returning(|_| Ok(()));
        mock_repo
            .expect_add_notifiers()
            .with(eq(network_id), function(|notifiers: &Vec<NotifierConfig>| notifiers.len() == 1))
            .once()
            .returning(|_, _| Ok(()));

        // Dummy config for AppConfig
        let config = AppConfig::builder()
            .network_id(network_id)
            .notifier_config_path(config_path.to_str().unwrap())
            .build();

        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Dummy path, won't be used
        let script_validator = create_script_validator();
        let initialization_service =
            InitializationService::new(config, Arc::new(mock_repo), abi_service, script_validator);

        let result = initialization_service.load_notifiers_from_file().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_load_notifiers_from_file_when_db_not_empty() {
        let temp_dir = tempdir().unwrap();
        let config_path = create_dummy_config_file(
            &temp_dir,
            "notifiers.yaml",
            create_test_notifier_config_str(),
        );
        let network_id = "testnet";

        let mut mock_repo = MockStateRepository::new();
        // Expect get_notifiers to be called and return non-empty
        mock_repo.expect_get_notifiers().with(eq(network_id)).once().returning(|_| {
            Ok(vec![NotifierConfig {
                name: "Dummy Notifier".to_string(),
                config: NotifierTypeConfig::Slack(SlackConfig {
                    slack_url: "http://dummy.url".to_string(),
                    message: NotificationMessage::default(),
                    retry_policy: HttpRetryConfig::default(),
                }),
                policy: None,
            }])
        }); // Return a dummy notifier
        // Expect clear_notifiers and add_notifiers to NOT be called
        mock_repo.expect_clear_notifiers().times(0);
        mock_repo.expect_add_notifiers().times(0);

        // Dummy config for AppConfig
        let config = AppConfig::builder()
            .network_id(network_id)
            .notifier_config_path(config_path.to_str().unwrap())
            .build();

        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]); // Dummy path, won't be used
        let script_validator = create_script_validator();
        let initialization_service =
            InitializationService::new(config, Arc::new(mock_repo), abi_service, script_validator);

        let result = initialization_service.load_notifiers_from_file().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_load_abis_from_monitors() {
        let temp_dir = tempdir().unwrap();
        let network_id = "testnet";
        let monitor = MonitorBuilder::new()
            .name("ABI Monitor")
            .network(network_id)
            .address("0x0000000000000000000000000000000000000001")
            .abi("test_abi")
            .build();

        let mut mock_repo = MockStateRepository::new();
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
        );

        let result = initialization_service.load_abis_from_monitors().await;
        assert!(result.is_ok());

        // Verify ABI was added to AbiService
        assert_eq!(abi_service.cache_size(), initial_abi_cache_size + 1);
        assert!(abi_service.is_monitored(&address!("0x0000000000000000000000000000000000000001")));
    }

    #[tokio::test]
    async fn test_load_abis_from_monitors_global_abi() {
        let temp_dir = tempdir().unwrap();
        let network_id = "testnet";
        let monitor = MonitorBuilder::new()
            .name("Global ABI Monitor")
            .network(network_id)
            .address("all") // This is the key for global ABI
            .abi("global_test_abi")
            .build();

        let mut mock_repo = MockStateRepository::new();
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
        );

        let result = initialization_service.load_abis_from_monitors().await;
        assert!(result.is_ok());

        // Verify ABI was added to AbiService's global cache
        assert!(abi_service.has_global_abi("global_test_abi"));
    }
}
