//! This module provides the `SupervisorBuilder` for constructing a
//! `Supervisor`.

use super::Supervisor;
use crate::{
    context::{AppContext, AppMetrics},
    persistence::traits::{AppRepository, KeyValueStore},
    providers::rpc::EvmRpcSource,
};

/// Marker for a provided component in the builder.
pub struct Provided<T>(T);

/// A builder for creating a `Supervisor` instance.
pub struct SupervisorBuilder<C, M> {
    context: C,
    app_metrics: M,
}

impl SupervisorBuilder<(), ()> {
    /// Creates a new, empty `SupervisorBuilder`.
    pub fn new() -> Self {
        Self { context: (), app_metrics: () }
    }
}

impl Default for SupervisorBuilder<(), ()> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C, M> SupervisorBuilder<C, M> {
    /// Sets the application context for the `Supervisor`.
    pub fn context<T: AppRepository + KeyValueStore + 'static>(
        self,
        context: AppContext<T>,
    ) -> SupervisorBuilder<Provided<AppContext<T>>, M> {
        SupervisorBuilder { context: Provided(context), app_metrics: self.app_metrics }
    }

    /// Sets the application metrics for the `Supervisor`.
    pub fn app_metrics(
        self,
        app_metrics: AppMetrics,
    ) -> SupervisorBuilder<C, Provided<AppMetrics>> {
        SupervisorBuilder { context: self.context, app_metrics: Provided(app_metrics) }
    }
}

impl<T: AppRepository + KeyValueStore + 'static>
    SupervisorBuilder<Provided<AppContext<T>>, Provided<AppMetrics>>
{
    /// Assembles and validates the components to build a `Supervisor`.
    ///
    /// This method uses the high-level services from `AppContext` to construct
    /// the `Supervisor`. The services are initialized by `AppContextBuilder`,
    /// ensuring consistent initialization between the supervisor and other
    /// application modes (e.g., dry-run).
    pub fn build(self) -> Supervisor<T> {
        let Provided(context) = self.context;
        let Provided(app_metrics) = self.app_metrics;

        tracing::debug!(rpc_urls = ?context.config.rpc_urls, "Initializing EVM data source...");
        let evm_data_source =
            EvmRpcSource::new(context.provider.clone(), context.monitor_manager.clone());
        tracing::info!(retry_policy = ?context.config.rpc_retry_config, "EVM data source initialized with fallback and retry policy.");

        // Construct the Supervisor with all its components from the context.
        Supervisor::new(
            context.config,
            context.repo,
            app_metrics,
            Box::new(evm_data_source),
            context.filtering_engine,
            context.alert_manager,
            context.monitor_manager,
            context.monitor_validator,
            context.action_dispatcher,
        )
    }
}
