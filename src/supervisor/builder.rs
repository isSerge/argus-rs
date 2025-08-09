//! This module provides the `SupervisorBuilder` for constructing a `Supervisor`.

use std::{fs, sync::Arc};

use crate::{
    abi::AbiService,
    config::AppConfig,
    engine::{block_processor::BlockProcessor, filtering::RhaiFilteringEngine},
    persistence::traits::StateRepository,
    providers::traits::DataSource,
};

use super::{Supervisor, SupervisorError};

/// A builder for creating a `Supervisor` instance.
#[derive(Default)]
pub struct SupervisorBuilder {
    config: Option<AppConfig>,
    state: Option<Arc<dyn StateRepository>>,
    abi_service: Option<Arc<AbiService>>,
    data_source: Option<Box<dyn DataSource>>,
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
    pub fn state(mut self, state: Arc<dyn StateRepository>) -> Self {
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

    /// Assembles and validates the components to build a `Supervisor`.
    ///
    /// This method performs the final "wiring" of the application's services.
    /// It ensures all required dependencies have been provided and then constructs
    /// the internal services, such as the `BlockProcessor` and `FilteringEngine`.
    pub async fn build(self) -> Result<Supervisor, SupervisorError> {
        let config = self.config.ok_or(SupervisorError::MissingConfig)?;
        let state = self.state.ok_or(SupervisorError::MissingStateRepository)?;
        let abi_service = self.abi_service.ok_or(SupervisorError::MissingAbiService)?;
        let data_source = self.data_source.ok_or(SupervisorError::MissingDataSource)?;

        // The FilteringEngine is created here, loading its initial set of monitors
        // from the database. This makes the database the single source of truth.
        tracing::debug!(network_id = %config.network_id, "Loading monitors from database for filtering engine...");
        let monitors = state.get_monitors(&config.network_id).await?;
        tracing::info!(count = monitors.len(), network_id = %config.network_id, "Loaded monitors from database for filtering engine.");

        // Load ABIs into the AbiService from the monitor's ABI file path.
        // This loop also serves as a validation step. If any ABI is invalid or
        // cannot be loaded, the supervisor will fail to build.
        for monitor in &monitors {
            if let (Some(address_str), Some(abi_path)) = (&monitor.address, &monitor.abi) {
                // 1. Parse the address
                let address = address_str.parse().map_err(|_| {
                    SupervisorError::InvalidConfiguration(format!(
                        "Invalid address '{}' for monitor '{}'",
                        address_str, monitor.name
                    ))
                })?;

                // 2. Read the ABI file content
                let abi_content = fs::read_to_string(abi_path).map_err(|e| {
                    SupervisorError::InvalidConfiguration(format!(
                        "Failed to read ABI file '{}' for monitor '{}': {}",
                        abi_path, monitor.name, e
                    ))
                })?;

                // 3. Parse the ABI JSON
                let abi =
                    serde_json::from_str::<alloy::json_abi::JsonAbi>(&abi_content).map_err(|e| {
                        SupervisorError::InvalidConfiguration(format!(
                            "Failed to parse ABI JSON from '{}' for monitor '{}': {}",
                            abi_path, monitor.name, e
                        ))
                    })?;

                // 4. Add the parsed ABI to the service
                abi_service.add_abi(address, &abi);
            }
        }

        // Construct the internal services.
        let block_processor = BlockProcessor::new(Arc::clone(&abi_service));

        let filtering_engine = RhaiFilteringEngine::new(monitors, config.rhai.clone())?;

        // Finally, construct the Supervisor with all its components.
        Ok(Supervisor::new(
            config,
            state,
            data_source,
            block_processor,
            Arc::new(filtering_engine),
        ))
    }
}
