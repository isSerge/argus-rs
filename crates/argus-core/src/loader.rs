//! Generic configuration loading traits and errors.
//!
//! This module provides the [`Loadable`] trait and [`LoaderError`] type.
//! The actual file-reading machinery (`ConfigLoader`, `load_config`) lives in
//! the root `argus` crate so that `argus-core` stays free of file-system
//! concerns beyond what is needed for error typing.

use config::ConfigError;
use serde::de::DeserializeOwned;
use thiserror::Error;

/// Errors that can occur during configuration loading.
#[derive(Debug, Error)]
pub enum LoaderError {
    /// Error when reading the configuration file.
    #[error("Failed to read configuration file: {0}")]
    IoError(#[from] std::io::Error),

    /// Error when parsing the configuration file.
    #[error("Failed to parse configuration: {0}")]
    ParseError(#[from] ConfigError),

    /// Error when the configuration format is unsupported.
    #[error("Unsupported configuration format")]
    UnsupportedFormat,

    /// Error when an expected environment variable is missing.
    #[error("Missing environment variable: {0}")]
    MissingEnvVar(String),
}

/// A trait for types that can be loaded from a configuration file.
pub trait Loadable: Sized + DeserializeOwned {
    /// The top-level key in the YAML file (e.g., "monitors").
    const KEY: &'static str;

    /// The specific error type for this loadable item.
    type Error: From<LoaderError>;

    /// Post-deserialization validation hook.
    ///
    /// Default implementation is a no-op.
    fn validate(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}
