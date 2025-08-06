use config::{Config, File};
use std::{fs, path::PathBuf};
use thiserror::Error;

use crate::models::monitor::Monitor;

/// Loads monitor configurations from a file.
pub struct MonitorLoader {
    path: PathBuf,
}

/// Errors that can occur while loading monitor configurations.
#[derive(Debug, Error)]
pub enum MonitorLoaderError {
    /// Error when reading the monitor configuration file.
    #[error("Failed to load monitor configuration: {0}")]
    IoError(std::io::Error),

    /// Error when parsing the monitor configuration file.
    #[error("Failed to parse monitor configuration: {0}")]
    ParseError(String),

    /// Error when the monitor configuration format is unsupported.
    #[error("Unsupported monitor configuration format")]
    UnsupportedFormat,
}

impl MonitorLoader {
    /// Creates a new `MonitorLoader` instance.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Loads the monitor configuration from the specified file.
    pub fn load(&self) -> Result<Vec<Monitor>, MonitorLoaderError> {
        // Validate YAML extension
        if !self.is_yaml_file() {
            return Err(MonitorLoaderError::UnsupportedFormat);
        }

        let config_str = fs::read_to_string(&self.path).map_err(MonitorLoaderError::IoError)?;
        let config: Vec<Monitor> = Config::builder()
            .add_source(File::from_str(&config_str, config::FileFormat::Yaml))
            .build()
            .map_err(|e| MonitorLoaderError::ParseError(e.to_string()))?
            .try_deserialize()
            .map_err(|e| MonitorLoaderError::ParseError(e.to_string()))?;
        Ok(config)
    }

    /// Checks if the file has a YAML extension.
    fn is_yaml_file(&self) -> bool {
        matches!(
            self.path.extension().and_then(|ext| ext.to_str()),
            Some("yaml") | Some("yml")
        )
    }
}
