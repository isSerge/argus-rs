//! Configuration module for Argus.

use config::{Config, ConfigError, File};
use serde::{Deserialize, Deserializer};
use url::Url;

/// Configuration for the retry backoff policy.
#[derive(Debug, Deserialize, Clone)]
pub struct RetryConfig {
    /// The maximum number of retries for a request.
    pub max_retry: u32,
    /// The initial backoff delay in milliseconds.
    pub backoff_ms: u64,
    /// The number of compute units per second to allow.
    pub compute_units_per_second: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retry: 10,
            backoff_ms: 1000,
            compute_units_per_second: 100,
        }
    }
}

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
    /// Optional retry configuration.
    #[serde(default)]
    pub retry_config: RetryConfig,
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

#[cfg(test)]
mod tests {
    use super::*;
    use config::Config;

    #[test]
    fn test_config_with_retry() {
        let yaml = "
            database_url: 'sqlite:test.db'
            rpc_urls: ['http://localhost:8545']
            network_id: 'testnet'
            retry_config:
              max_retry: 5
              backoff_ms: 500
              compute_units_per_second: 50
        ";

        let builder =
            Config::builder().add_source(config::File::from_str(yaml, config::FileFormat::Yaml));
        let app_config: AppConfig = builder.build().unwrap().try_deserialize().unwrap();

        assert_eq!(app_config.retry_config.max_retry, 5);
        assert_eq!(app_config.retry_config.backoff_ms, 500);
        assert_eq!(app_config.retry_config.compute_units_per_second, 50);
        assert_eq!(app_config.rpc_urls[0].to_string(), "http://localhost:8545/");
    }

    #[test]
    fn test_config_without_retry_uses_default() {
        let yaml = "
            database_url: 'sqlite:test.db'
            rpc_urls: ['http://localhost:8545']
            network_id: 'testnet'
        ";

        let builder =
            Config::builder().add_source(config::File::from_str(yaml, config::FileFormat::Yaml));
        let app_config: AppConfig = builder.build().unwrap().try_deserialize().unwrap();

        let default_retry_config = RetryConfig::default();
        assert_eq!(
            app_config.retry_config.max_retry,
            default_retry_config.max_retry
        );
        assert_eq!(
            app_config.retry_config.backoff_ms,
            default_retry_config.backoff_ms
        );
        assert_eq!(
            app_config.retry_config.compute_units_per_second,
            default_retry_config.compute_units_per_second
        );
    }
}
