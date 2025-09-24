//! This module provides the `SupervisorBuilder` for constructing a
//! `Supervisor`.

use std::{collections::HashMap, sync::Arc};

use super::{Supervisor, SupervisorError};
use crate::{
    abi::AbiService,
    config::{ActionConfig, AppConfig},
    engine::{
        action_handler::ActionHandler, filtering::RhaiFilteringEngine, js_client,
        match_manager::MatchManager, rhai::RhaiCompiler,
    },
    http_client::HttpClientPool,
    models::notifier::NotifierConfig,
    monitor::MonitorManager,
    notification::NotificationService,
    persistence::{sqlite::SqliteStateRepository, traits::StateRepository},
    providers::rpc::{EvmRpcSource, create_provider},
};

/// A builder for creating a `Supervisor` instance.
#[derive(Default)]
pub struct SupervisorBuilder {
    config: Option<AppConfig>,
    state: Option<Arc<SqliteStateRepository>>,
    abi_service: Option<Arc<AbiService>>,
    script_compiler: Option<Arc<RhaiCompiler>>,
    actions: Option<Vec<ActionConfig>>,
    action_handler: Option<Arc<ActionHandler>>,
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

    /// Sets the Rhai script compiler for the `Supervisor`.
    pub fn script_compiler(mut self, script_compiler: Arc<RhaiCompiler>) -> Self {
        self.script_compiler = Some(script_compiler);
        self
    }

    /// Sets the actions configuration for the `Supervisor`.
    pub fn actions(mut self, actions: Vec<ActionConfig>) -> Self {
        self.actions = Some(actions);
        self
    }

    #[cfg(test)]
    pub fn action_handler(mut self, action_handler: Arc<ActionHandler>) -> Self {
        self.action_handler = Some(action_handler);
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
        let script_compiler = self.script_compiler.ok_or(SupervisorError::MissingScriptCompiler)?;

        // The FilteringEngine is created here, loading its initial set of monitors
        // from the database. This makes the database the single source of truth.
        tracing::debug!(network_id = %config.network_id, "Loading monitors from database for filtering engine...");
        let monitors = state.get_monitors(&config.network_id).await?;
        tracing::info!(count = monitors.len(), network_id = %config.network_id, "Loaded monitors from database for filtering engine.");

        let monitor_manager =
            Arc::new(MonitorManager::new(monitors, script_compiler.clone(), abi_service.clone()));

        tracing::debug!(rpc_urls = ?config.rpc_urls, "Initializing EVM data source...");
        let provider = create_provider(config.rpc_urls.clone(), config.rpc_retry_config.clone())?;
        let evm_data_source = EvmRpcSource::new(provider, monitor_manager.clone());
        tracing::info!(retry_policy = ?config.rpc_retry_config, "EVM data source initialized with fallback and retry policy.");

        // Load notifiers from the database for the NotificationService.
        tracing::debug!(network_id = %config.network_id, "Loading notifiers from database for notification service...");
        let notifiers = state.get_notifiers(&config.network_id).await?;
        tracing::info!(count = notifiers.len(), network_id = %config.network_id, "Loaded notifiers from database for notification service.");

        // Construct the internal services.
        let filtering_engine = RhaiFilteringEngine::new(
            abi_service.clone(),
            script_compiler,
            config.rhai.clone(),
            monitor_manager.clone(),
        );
        let http_client_pool = Arc::new(HttpClientPool::new(config.http_base_config.clone()));

        // Set up the NotificationService and MatchManager
        let notifiers_map: Arc<HashMap<String, NotifierConfig>> =
            Arc::new(notifiers.into_iter().map(|t| (t.name.clone(), t)).collect());
        let notification_service =
            Arc::new(NotificationService::new(notifiers_map.clone(), http_client_pool));
        let actions = self
            .actions
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.name.clone(), a))
            .collect::<HashMap<_, _>>();

        // If an ActionHandler was provided, use it (during tests); otherwise, create a
        // default one (in production).
        let action_handler = if let Some(action_handler) = self.action_handler {
            Some(action_handler)
        // If no actions are configured, skip creating the ActionHandler.
        } else if !actions.is_empty() {
            let executor_client = Arc::new(
                js_client::JsExecutorClient::new()
                    .await
                    .map_err(SupervisorError::JsExecutorClient)?,
            );
            Some(Arc::new(ActionHandler::new(
                Arc::new(actions),
                monitor_manager.clone(),
                executor_client,
            )))
        } else {
            None
        };

        let match_manager = Arc::new(MatchManager::new(
            notification_service,
            state.clone(),
            notifiers_map,
            action_handler,
        ));

        // Finally, construct the Supervisor with all its components.
        Ok(Supervisor::new(
            config,
            state,
            Box::new(evm_data_source),
            Arc::new(filtering_engine),
            match_manager,
            monitor_manager,
        ))
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::{
        config::RhaiConfig,
        engine::action_handler,
        models::monitor::MonitorConfig,
        test_helpers::{create_test_abi_service, create_test_monitor_manager},
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
            ..Default::default()
        };

        let state_repo = Arc::new(setup_test_db().await);
        state_repo.add_monitors(network_id, vec![monitor]).await.unwrap();

        let (abi_service, _) = create_test_abi_service(&dir, &[(abi_name, "[]")]);

        let builder = SupervisorBuilder::new()
            .config(app_config)
            .state(state_repo)
            .abi_service(abi_service)
            .script_compiler(Arc::new(RhaiCompiler::new(RhaiConfig::default())));

        let result = builder.build().await;
        assert!(result.is_ok(), "Expected build to succeed, but got error: {:?}", result.err());
    }

    #[tokio::test]
    async fn build_fails_if_config_is_missing() {
        let dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&dir, &[]);
        let state_repo = Arc::new(setup_test_db().await);
        let builder = SupervisorBuilder::new().state(state_repo).abi_service(abi_service);

        let result = builder.build().await;
        assert!(matches!(result, Err(SupervisorError::MissingConfig)));
    }

    #[tokio::test]
    async fn build_fails_if_state_repository_is_missing() {
        let dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&dir, &[]);
        let builder =
            SupervisorBuilder::new().config(AppConfig::default()).abi_service(abi_service);

        let result = builder.build().await;
        assert!(matches!(result, Err(SupervisorError::MissingStateRepository)));
    }

    #[tokio::test]
    async fn build_fails_if_abi_service_is_missing() {
        let state_repo = Arc::new(setup_test_db().await);
        let builder = SupervisorBuilder::new().config(AppConfig::default()).state(state_repo);

        let result = builder.build().await;
        assert!(matches!(result, Err(SupervisorError::MissingAbiService)));
    }

    #[tokio::test]
    async fn builder_can_add_actions_and_build() {
        let app_config = AppConfig::builder()
            .rpc_urls(vec![
                "http://localhost:8545".parse().unwrap(), // Cannot be empty to create provider
            ])
            .build();
        let dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&dir, &[]);
        let state_repo = Arc::new(setup_test_db().await);
        let actions = vec![
            ActionConfig { name: "action1".into(), file: "actions/action1.js".into() },
            ActionConfig { name: "action2".into(), file: "actions/action2.js".into() },
        ];

        let monitor_manager = create_test_monitor_manager(vec![]); // Empty monitors for this test
        let mock_js_client = Arc::new(js_client::MockJsClient::new());
        let action_handler = Arc::new(action_handler::ActionHandler::new(
            Arc::new(
                actions.iter().map(|a| (a.name.clone(), a.clone())).collect::<HashMap<_, _>>(),
            ),
            monitor_manager,
            mock_js_client,
        ));

        let builder = SupervisorBuilder::new()
            .config(app_config)
            .state(state_repo)
            .abi_service(abi_service)
            .script_compiler(Arc::new(RhaiCompiler::new(RhaiConfig::default())))
            .actions(actions.clone())
            .action_handler(action_handler);

        let supervisor = builder.build().await;

        assert!(
            supervisor.is_ok(),
            "Expected build to succeed, but got error: {:?}",
            supervisor.err()
        );
    }

    #[tokio::test]
    async fn build_succeeds_with_action_handler() {
        let app_config = AppConfig::builder()
            .rpc_urls(vec![
                "http://localhost:8545".parse().unwrap(), // Cannot be empty to create provider
            ])
            .build();
        let dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&dir, &[]);
        let state_repo = Arc::new(setup_test_db().await);
        let actions = vec![
            ActionConfig { name: "action1".into(), file: "actions/action1.js".into() },
            ActionConfig { name: "action2".into(), file: "actions/action2.js".into() },
        ];

        let monitor_manager = create_test_monitor_manager(vec![]); // Empty monitors for this test
        let mock_js_client = Arc::new(js_client::MockJsClient::new());
        let action_handler = Arc::new(action_handler::ActionHandler::new(
            Arc::new(
                actions.iter().map(|a| (a.name.clone(), a.clone())).collect::<HashMap<_, _>>(),
            ),
            monitor_manager,
            mock_js_client,
        ));

        let builder = SupervisorBuilder::new()
            .config(app_config)
            .state(state_repo)
            .abi_service(abi_service)
            .script_compiler(Arc::new(RhaiCompiler::new(RhaiConfig::default())))
            .actions(actions.clone())
            .action_handler(action_handler);

        let supervisor = builder.build().await;

        assert!(
            supervisor.is_ok(),
            "Expected build to succeed, but got error: {:?}",
            supervisor.err()
        );
    }
}
