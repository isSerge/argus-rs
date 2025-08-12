//! Loads and validates trigger configurations from a YAML file.

use super::loader::{ConfigLoader, LoaderError};
use crate::{models::trigger::TriggerConfig, notification::error::NotificationError};
use std::path::PathBuf;
use thiserror::Error;

/// Loads trigger configurations from a file.
pub struct TriggerLoader {
    path: PathBuf,
}

/// Errors that can occur while loading trigger configurations.
#[derive(Debug, Error)]
pub enum TriggerLoaderError {
    /// An error occurred during the loading process.
    #[error("Failed to load trigger configuration: {0}")]
    Loader(#[from] LoaderError),

    /// The trigger configuration is invalid.
    #[error("Invalid trigger configuration: {0}")]
    ValidationError(#[from] NotificationError),
}

impl TriggerLoader {
    /// Creates a new `TriggerLoader` instance.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Loads and validates the trigger configurations from the specified file.
    pub fn load(&self) -> Result<Vec<TriggerConfig>, TriggerLoaderError> {
        let loader = ConfigLoader::new(self.path.clone());
        let triggers: Vec<TriggerConfig> = loader.load("triggers")?;

        for trigger_config in &triggers {
            trigger_config.config.validate()?;
        }

        Ok(triggers)
    }
}
