use thiserror::Error;

use crate::{
    abi::repository::AbiRepositoryError, persistence::error::PersistenceError,
    providers::rpc::ProviderError,
};

/// Errors that can occur during application context initialization.
#[derive(Debug, Error)]
pub enum AppContextError {
    /// Configuration error.
    #[error("Config error: {0}")]
    Config(#[from] config::ConfigError),

    /// Persistence error.
    #[error("Persistence error: {0}")]
    Persistence(#[from] PersistenceError),

    /// ABI repository error.
    #[error("ABI repository error: {0}")]
    AbiRepository(#[from] AbiRepositoryError),

    /// Provider error.
    #[error("Provider error: {0}")]
    Provider(#[from] ProviderError),

    /// Initialization error.
    #[error("Initialization error: {0}")]
    Initialization(#[from] InitializationError),
}

/// Errors that can occur during specific initialization steps.
#[derive(Debug, Error)]
pub enum InitializationError {
    /// Failed to load monitors from file.
    #[error("Failed to load monitors from file: {0}")]
    MonitorLoad(String),

    /// Failed to load action from file.
    #[error("Failed to load action from file: {0}")]
    ActionLoad(String),

    /// Failed to load ABIs from monitors.
    #[error("Failed to load ABIs from monitors: {0}")]
    AbiLoad(String),

    /// Failed to initialize block state.
    #[error("Failed to initialize block state: {0}")]
    BlockStateInitialization(String),
}
