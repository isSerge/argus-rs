//! The SupervisorBuilder is used to construct a Supervisor instance with all necessary components.

use std::sync::Arc;

use crate::{
    abi::AbiService,
    config::AppConfig,
    engine::{block_processor::BlockProcessor, filtering::RhaiFilteringEngine},
    persistence::traits::StateRepository,
    providers::traits::DataSource,
};

use super::{Supervisor, SupervisorError};

/// The SupervisorBuilder struct holds the configuration and components needed to build a Supervisor instance.
#[derive(Default)]
pub struct SupervisorBuilder {
    config: Option<AppConfig>,
    state: Option<Arc<dyn StateRepository>>,
    abi_service: Option<Arc<AbiService>>,
    data_source: Option<Box<dyn DataSource>>,
}

impl SupervisorBuilder {
    /// Creates a new SupervisorBuilder instance.
    pub fn new() -> Self {
        Self {
            config: None,
            state: None,
            abi_service: None,
            data_source: None,
        }
    }

    /// Sets the configuration for the Supervisor.
    pub fn config(mut self, config: AppConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Sets the state repository for the Supervisor.
    pub fn state(mut self, state: Arc<dyn StateRepository>) -> Self {
        self.state = Some(state);
        self
    }

    /// Sets the ABI service for the Supervisor.
    pub fn abi_service(mut self, abi_service: Arc<AbiService>) -> Self {
        self.abi_service = Some(abi_service);
        self
    }

    /// Sets the data source for the Supervisor.
    pub fn data_source(mut self, data_source: Box<dyn DataSource>) -> Self {
        self.data_source = Some(data_source);
        self
    }

    /// Builds the Supervisor instance, validating all required components are set.
    pub async fn build(self) -> Result<Supervisor, SupervisorError> {
        let config = self.config.ok_or(SupervisorError::MissingConfig)?;
        let state = self.state.ok_or(SupervisorError::MissingStateRepository)?;
        let abi_service = self.abi_service.ok_or(SupervisorError::MissingAbiService)?;
        let data_source = self.data_source.ok_or(SupervisorError::MissingDataSource)?;
        let block_processor = BlockProcessor::new(abi_service);

        // Always load the monitors for the filtering engine from the database,
        // as it's the single source of truth for the running application.
        tracing::debug!(network_id = %config.network_id, "Loading monitors from database for filtering engine...");
        let monitors = state.get_monitors(&config.network_id).await?;
        tracing::info!(count = monitors.len(), network_id = %config.network_id, "Loaded monitors from database for filtering engine.");

        let filtering_engine = RhaiFilteringEngine::new(monitors, config.rhai.clone());

        Ok(Supervisor::new(
            config,
            state,
            data_source,
            block_processor,
            Arc::new(filtering_engine),
        ))
    }
}
