//! This module provides the `SupervisorBuilder` for constructing a
//! `Supervisor`.

use std::{collections::HashMap, sync::Arc};

use super::{Supervisor, SupervisorError};
use crate::{
    abi::AbiService,
    config::AppConfig,
    engine::{
        alert_manager::AlertManager, block_processor::BlockProcessor,
        filtering::RhaiFilteringEngine, rhai::RhaiCompiler,
    },
    http_client::HttpClientPool,
    models::notifier::NotifierConfig,
    monitor::MonitorManager,
    notification::NotificationService,
    persistence::{sqlite::SqliteStateRepository, traits::StateRepository},
    providers::traits::DataSource,
};

/// A builder for creating a `Supervisor` instance.
#[derive(Default)]
pub struct SupervisorBuilder {
    config: Option<AppConfig>,
    state: Option<Arc<SqliteStateRepository>>,
    abi_service: Option<Arc<AbiService>>,
    data_source: Option<Box<dyn DataSource>>,
    script_compiler: Option<Arc<RhaiCompiler>>,
}

impl SupervisorBuilder {
    /// Creates a new, empty `SupervisorBuilder`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the application configuration for the `Supervisor`.
    pub fn config(mut self, config: AppConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Sets the state repository (database connection) for the `Supervisor`.
    pub fn state(mut self, state: Arc<SqliteStateRepository>) -> Self {
        self.state = Some(state);
        self
    }

    /// Sets the ABI service for the `Supervisor`.
    pub fn abi_service(mut self, abi_service: Arc<AbiService>) -> Self {
        self.abi_service = Some(abi_service);
        self
    }

    /// Sets the data source (e.g., RPC client) for the `Supervisor`.
    pub fn data_source(mut self, data_source: Box<dyn DataSource>) -> Self {
        self.data_source = Some(data_source);
        self
    }

    /// Sets the Rhai script compiler for the `Supervisor`.
    pub fn script_compiler(mut self, script_compiler: Arc<RhaiCompiler>) -> Self {
        self.script_compiler = Some(script_compiler);
        self
    }

    /// Assembles and validates the components to build a `Supervisor`.
    ///
    /// This method performs the final "wiring" of the application's services.
    /// It ensures all required dependencies have been provided and then
    /// constructs the internal services, such as the `BlockProcessor` and
    /// `FilteringEngine`.
    pub async fn build(self) -> Result<Supervisor, SupervisorError> {
        let config = self.config.ok_or(SupervisorError::MissingConfig)?;
        let state = self.state.ok_or(SupervisorError::MissingStateRepository)?;
        let abi_service = self.abi_service.ok_or(SupervisorError::MissingAbiService)?;
        let data_source = self.data_source.ok_or(SupervisorError::MissingDataSource)?;
        let script_compiler = self.script_compiler.ok_or(SupervisorError::MissingScriptCompiler)?;

        // The FilteringEngine is created here, loading its initial set of monitors
        // from the database. This makes the database the single source of truth.
        tracing::debug!(network_id = %config.network_id, "Loading monitors from database for filtering engine...");
        let monitors = state.get_monitors(&config.network_id).await?;
        tracing::info!(count = monitors.len(), network_id = %config.network_id, "Loaded monitors from database for filtering engine.");

        let monitor_manager =
            Arc::new(MonitorManager::new(monitors, script_compiler.clone(), abi_service.clone()));

        // Load notifiers from the database for the NotificationService.
        tracing::debug!(network_id = %config.network_id, "Loading notifiers from database for notification service...");
        let notifiers = state.get_notifiers(&config.network_id).await?;
        tracing::info!(count = notifiers.len(), network_id = %config.network_id, "Loaded notifiers from database for notification service.");

        // Construct the internal services.
        let block_processor =
            BlockProcessor::new(Arc::clone(&abi_service), monitor_manager.clone());
        let filtering_engine =
            RhaiFilteringEngine::new(script_compiler, config.rhai.clone(), monitor_manager.clone());
        let http_client_pool = Arc::new(HttpClientPool::new(config.http_base_config.clone()));

        // Set up the NotificationService and AlertManager
        let notifiers_map: Arc<HashMap<String, NotifierConfig>> =
            Arc::new(notifiers.into_iter().map(|t| (t.name.clone(), t)).collect());
        let notification_service =
            Arc::new(NotificationService::new(notifiers_map.clone(), http_client_pool));
        let alert_manager =
            Arc::new(AlertManager::new(notification_service, state.clone(), notifiers_map));

        // Finally, construct the Supervisor with all its components.
        Ok(Supervisor::new(
            config,
            state,
            data_source,
            block_processor,
            Arc::new(filtering_engine),
            alert_manager,
            monitor_manager,
        ))
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::{
        config::RhaiConfig, models::monitor::MonitorConfig, providers::traits::MockDataSource,
        test_helpers::create_test_abi_service,
    };

    async fn setup_test_db() -> SqliteStateRepository {
        let repo = SqliteStateRepository::new("sqlite::memory:")
            .await
            .expect("Failed to connect to in-memory db");
        repo.run_migrations().await.expect("Failed to run migrations");
        repo
    }

    #[tokio::test]
    async fn build_succeeds_with_valid_monitors() {
        let network_id = "testnet";
        let abi_name = "abi";
        let dir = tempdir().unwrap();

        let monitor = MonitorConfig {
            name: "Valid Monitor".into(),
            network: network_id.into(),
            address: Some("0x0000000000000000000000000000000000000001".to_string()),
            abi: Some(abi_name.to_string()),
            filter_script: "true".to_string(),
            notifiers: vec![],
        };

        let state_repo = Arc::new(setup_test_db().await);
        state_repo.add_monitors(network_id, vec![monitor]).await.unwrap();

        let (abi_service, _) = create_test_abi_service(&dir, &[(abi_name, "[]")]);

        let builder = SupervisorBuilder::new()
            .config(AppConfig::default())
            .state(state_repo)
            .abi_service(abi_service)
            .script_compiler(Arc::new(RhaiCompiler::new(RhaiConfig::default())))
            .data_source(Box::new(MockDataSource::new()));

        let result = builder.build().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn build_fails_if_config_is_missing() {
        let dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&dir, &[]);
        let state_repo = Arc::new(setup_test_db().await);
        let builder = SupervisorBuilder::new()
            .state(state_repo)
            .abi_service(abi_service)
            .data_source(Box::new(MockDataSource::new()));

        let result = builder.build().await;
        assert!(matches!(result, Err(SupervisorError::MissingConfig)));
    }

    #[tokio::test]
    async fn build_fails_if_state_repository_is_missing() {
        let dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&dir, &[]);
        let builder = SupervisorBuilder::new()
            .config(AppConfig::default())
            .abi_service(abi_service)
            .data_source(Box::new(MockDataSource::new()));

        let result = builder.build().await;
        assert!(matches!(result, Err(SupervisorError::MissingStateRepository)));
    }

    #[tokio::test]
    async fn build_fails_if_abi_service_is_missing() {
        let state_repo = Arc::new(setup_test_db().await);
        let builder = SupervisorBuilder::new()
            .config(AppConfig::default())
            .state(state_repo)
            .data_source(Box::new(MockDataSource::new()));

        let result = builder.build().await;
        assert!(matches!(result, Err(SupervisorError::MissingAbiService)));
    }

    #[tokio::test]
    async fn build_fails_if_data_source_is_missing() {
        let dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&dir, &[]);
        let state_repo = Arc::new(setup_test_db().await);
        let builder = SupervisorBuilder::new()
            .config(AppConfig::default())
            .state(state_repo)
            .abi_service(abi_service);

        let result = builder.build().await;
        assert!(matches!(result, Err(SupervisorError::MissingDataSource)));
    }
}
