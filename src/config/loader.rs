//! Generic configuration loader for loading items from a YAML file.

use config::{Config, File, FileFormat};
use serde::de::DeserializeOwned;
use std::{fs, path::PathBuf};
use thiserror::Error;

/// A generic container for items loaded from a YAML file.
/// The top-level key in the YAML file should match the `key` field.
pub struct ConfigFile<T> {
    pub key: String,
    pub items: Vec<T>,
}

/// A generic loader for YAML files.
pub struct ConfigLoader {
    path: PathBuf,
}

/// Errors that can occur during configuration loading.
#[derive(Debug, Error)]
pub enum LoaderError {
    #[error("Failed to read configuration file: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Failed to parse configuration: {0}")]
    ParseError(#[from] config::ConfigError),

    #[error("Unsupported configuration format")]
    UnsupportedFormat,
}

impl ConfigLoader {
    /// Creates a new `ConfigLoader`.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Loads a vector of items from the YAML file.
    /// The generic type `T` must be deserializable.
    /// The `key` parameter specifies the top-level key in the YAML file
    /// that holds the list of items (e.g., "monitors", "triggers").
    pub fn load<T: DeserializeOwned>(&self, key: &str) -> Result<Vec<T>, LoaderError> {
        if !self.is_yaml_file() {
            return Err(LoaderError::UnsupportedFormat);
        }

        let config_str = fs::read_to_string(&self.path)?;

        let config = Config::builder()
            .add_source(File::from_str(&config_str, FileFormat::Yaml))
            .build()?;

        let items = config.get(key)?;

        Ok(items)
    }

    /// Checks if the file has a YAML extension.
    fn is_yaml_file(&self) -> bool {
        matches!(
            self.path.extension().and_then(|ext| ext.to_str()),
            Some("yaml") | Some("yml")
        )
    }
}
