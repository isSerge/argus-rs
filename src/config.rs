//! Configuration module for Argus.

use config::{Config, ConfigError, File};
use serde::{Deserialize, Deserializer};
use url::Url;

/// Application configuration for Argus.
#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    /// Database URL for the SQLite database.
    pub database_url: String,
    /// RPC URLs for the Ethereum node.
    #[serde(deserialize_with = "deserialize_urls")]
    pub rpc_urls: Vec<Url>,
    /// Network ID for the Ethereum network.
    pub network_id: String,
}

/// Custom deserializer for a vector of URLs.
fn deserialize_urls<'de, D>(deserializer: D) -> Result<Vec<Url>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = Vec::<String>::deserialize(deserializer)?;
    s.into_iter()
        .map(|url_str| Url::parse(&url_str).map_err(serde::de::Error::custom))
        .collect()
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
