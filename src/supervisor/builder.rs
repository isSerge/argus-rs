//! This module provides the `SupervisorBuilder` for constructing a
//! `Supervisor`.

use super::{Supervisor, SupervisorError};
use crate::{
    context::{AppContext, AppMetrics},
    persistence::traits::{AppRepository, KeyValueStore},
    providers::rpc::EvmRpcSource,
};

/// A builder for creating a `Supervisor` instance.
pub struct SupervisorBuilder<T: AppRepository + KeyValueStore> {
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
    /// This method uses the high-level services from `AppContext` to construct
    /// the `Supervisor`. The services are initialized by `AppContextBuilder`,
    /// ensuring consistent initialization between the supervisor and other
    /// application modes (e.g., dry-run).
    pub async fn build(self) -> Result<Supervisor<T>, SupervisorError> {
        let context = self.context.ok_or(SupervisorError::MissingConfig)?;
        let app_metrics = self.app_metrics.ok_or(SupervisorError::MissingAppMetrics)?;

        tracing::debug!(rpc_urls = ?context.config.rpc_urls, "Initializing EVM data source...");
        let evm_data_source =
            EvmRpcSource::new(context.provider.clone(), context.monitor_manager.clone());
        tracing::info!(retry_policy = ?context.config.rpc_retry_config, "EVM data source initialized with fallback and retry policy.");

        // Construct the Supervisor with all its components from the context.
        Ok(Supervisor::new(
            context.config,
            context.repo,
            app_metrics,
            Box::new(evm_data_source),
            context.filtering_engine,
            context.alert_manager,
            context.monitor_manager,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        context::AppContextBuilder, models::monitor::MonitorConfig,
        persistence::sqlite::SqliteStateRepository,
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
        let state_repo = setup_test_db().await;

        let network_id = "testnet";
        let monitor = MonitorConfig {
            name: "Valid Monitor".into(),
            network: network_id.into(),
            address: None,
            abi: None,
            filter_script: "true".to_string(),
            actions: vec![],
        };

        state_repo.add_monitors(network_id, vec![monitor]).await.unwrap();

        // Use AppContextBuilder to create a properly initialized context
        let context = AppContextBuilder::new(None, None)
            .database_url("sqlite::memory:".to_string())
            .build()
            .await
            .expect("Failed to build context");

        let builder = SupervisorBuilder::new().app_metrics(AppMetrics::default()).context(context);

        let result = builder.build().await;
        assert!(result.is_ok(), "Expected build to succeed, but got error: {:?}", result.err());
    }

    #[tokio::test]
    async fn build_fails_if_config_is_missing() {
        let builder = SupervisorBuilder::<SqliteStateRepository>::new();

        let result: Result<Supervisor<SqliteStateRepository>, SupervisorError> =
            builder.build().await;
        assert!(matches!(result, Err(SupervisorError::MissingConfig)));
    }
}
