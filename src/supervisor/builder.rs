//! This module provides the `SupervisorBuilder` for constructing a
//! `Supervisor`.

use std::{collections::HashMap, sync::Arc};

use super::{Supervisor, SupervisorError};
use crate::{
    actions::ActionDispatcher,
    context::{AppContext, AppMetrics},
    engine::{alert_manager::AlertManager, filtering::RhaiFilteringEngine},
    http_client::HttpClientPool,
    models::action::ActionConfig,
    monitor::MonitorManager,
    persistence::traits::{AppRepository, KeyValueStore},
    providers::rpc::EvmRpcSource,
};

/// A builder for creating a `Supervisor` instance.
pub struct SupervisorBuilder<T: AppRepository> {
    context: Option<AppContext<T>>,
    app_metrics: Option<AppMetrics>,
}

impl<T: AppRepository + KeyValueStore + 'static> Default for SupervisorBuilder<T> {
    fn default() -> Self {
        Self { context: None, app_metrics: None }
    }
}

impl<T: AppRepository + KeyValueStore + 'static> SupervisorBuilder<T> {
    /// Creates a new, empty `SupervisorBuilder`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the application context for the `Supervisor`.
    pub fn context(mut self, context: AppContext<T>) -> Self {
        self.context = Some(context);
        self
    }

    /// Sets the application metrics for the `Supervisor`.
    pub fn app_metrics(mut self, app_metrics: AppMetrics) -> Self {
        self.app_metrics = Some(app_metrics);
        self
    }

    /// Assembles and validates the components to build a `Supervisor`.
    ///
    /// This method performs the final "wiring" of the application's services.
    /// It ensures all required dependencies have been provided and then
    /// constructs the internal services, such as the `BlockProcessor` and
    /// `FilteringEngine`.
    pub async fn build(self) -> Result<Supervisor<T>, SupervisorError> {
        let context = self.context.ok_or(SupervisorError::MissingConfig)?;
        let app_metrics = self.app_metrics.ok_or(SupervisorError::MissingAppMetrics)?;
        let config = context.config;
        let state = context.repo;
        let abi_service = context.abi_service;
        let script_compiler = context.script_compiler;
        let provider = context.provider;

        // The FilteringEngine is created here, loading its initial set of monitors
        // from the database. This makes the database the single source of truth.
        tracing::debug!(network_id = %config.network_id, "Loading monitors from database for filtering engine...");
        let monitors = state.get_monitors(&config.network_id).await?;
        tracing::info!(count = monitors.len(), network_id = %config.network_id, "Loaded monitors from database for filtering engine.");

        let monitor_manager =
            Arc::new(MonitorManager::new(monitors, script_compiler.clone(), abi_service.clone()));

        tracing::debug!(rpc_urls = ?config.rpc_urls, "Initializing EVM data source...");
        let evm_data_source = EvmRpcSource::new(provider, monitor_manager.clone());
        tracing::info!(retry_policy = ?config.rpc_retry_config, "EVM data source initialized with fallback and retry policy.");

        // Load actions from the database for the ActionDispatcher.
        tracing::debug!(network_id = %config.network_id, "Loading actions from database for notification service...");
        let actions = state.get_actions(&config.network_id).await?;
        tracing::info!(count = actions.len(), network_id = %config.network_id, "Loaded actions from database for notification service.");

        // Construct the internal services.
        let filtering_engine = RhaiFilteringEngine::new(
            abi_service.clone(),
            script_compiler,
            config.rhai.clone(),
            monitor_manager.clone(),
        );
        let http_client_pool = Arc::new(HttpClientPool::new(config.http_base_config.clone()));

        // Set up the ActionDispatcher and AlertManager
        let actions_map: Arc<HashMap<String, ActionConfig>> =
            Arc::new(actions.into_iter().map(|t| (t.name.clone(), t)).collect());
        let action_dispatcher =
            Arc::new(ActionDispatcher::new(actions_map.clone(), http_client_pool).await?);
        let alert_manager =
            Arc::new(AlertManager::new(action_dispatcher, state.clone(), actions_map));

        // Finally, construct the Supervisor with all its components.
        Ok(Supervisor::new(
            config,
            state,
            app_metrics,
            Box::new(evm_data_source),
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
        actions::template::TemplateService, config::{AppConfig, RhaiConfig}, context::AppContext, engine::rhai::RhaiCompiler, models::monitor::MonitorConfig, persistence::sqlite::SqliteStateRepository, test_helpers::{create_test_abi_service, mock_provider}
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
        let (provider, _) = mock_provider();

        let network_id = "testnet";
        let abi_name = "abi";
        let dir = tempdir().unwrap();

        let app_config = AppConfig::builder()
            .rpc_urls(vec![
                "http://localhost:8545".parse().unwrap(), // Cannot be empty to create provider
            ])
            .build();

        let monitor = MonitorConfig {
            name: "Valid Monitor".into(),
            network: network_id.into(),
            address: Some("0x0000000000000000000000000000000000000001".to_string()),
            abi: Some(abi_name.to_string()),
            filter_script: "true".to_string(),
            actions: vec![],
        };

        let state_repo = Arc::new(setup_test_db().await);
        state_repo.add_monitors(network_id, vec![monitor]).await.unwrap();

        let (abi_service, _) = create_test_abi_service(&dir, &[(abi_name, "[]")]);

        let context = AppContext {
            config: app_config,
            repo: state_repo,
            abi_service,
            script_compiler: Arc::new(RhaiCompiler::new(RhaiConfig::default())),
            provider,
            template_service: Arc::new(TemplateService::new()),
        };

        let builder = SupervisorBuilder::new().app_metrics(AppMetrics::default()).context(context);

        let result = builder.build().await;
        assert!(result.is_ok(), "Expected build to succeed, but got error: {:?}", result.err());
    }

    #[tokio::test]
    async fn build_fails_if_config_is_missing() {
        let builder = SupervisorBuilder::<SqliteStateRepository>::new();

        let result: Result<Supervisor<SqliteStateRepository>, SupervisorError> = builder.build().await;
        assert!(matches!(result, Err(SupervisorError::MissingConfig)));
    }
}
