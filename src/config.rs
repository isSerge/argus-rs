//! Configuration module for Argus.

use config::{Config, ConfigError, File};
use serde::Deserialize;

/// Application configuration for Argus.
#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    /// Database URL for the SQLite database.
    pub database_url: String,
    /// RPC URLs for the Ethereum node.
    pub rpc_urls: Vec<String>,
    /// Network ID for the Ethereum network.
    pub network_id: String,
}

impl AppConfig {
    /// Creates a new `AppConfig` by reading from the configuration file.
    pub fn new() -> Result<Self, ConfigError> {
        let s = Config::builder()
            .add_source(File::with_name("config.yaml"))
            .build()?;
        s.try_deserialize()
    }
}
