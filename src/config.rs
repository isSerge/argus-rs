//! Configuration module for Argus.

use config::{Config, ConfigError, File};
use serde::{Deserialize, Deserializer};
use url::Url;
use std::time::Duration;

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

    /// The size of the block chunk to process at once.
    pub block_chunk_size: u64,

    /// The interval in milliseconds to poll for new blocks.
    pub polling_interval_ms: u64,

    /// Number of confirmation blocks to wait for before processing.
    pub confirmation_blocks: u64,

    /// The maximum time in seconds to wait for graceful shutdown.
    #[serde(default = "default_shutdown_timeout")]
    pub shutdown_timeout_secs: u64,

    /// Rhai script execution configuration.
    #[serde(default)]
    pub rhai: RhaiConfig,
}

/// Configuration for Rhai script execution including security limits and other settings
#[derive(Debug, Deserialize, Clone)]
pub struct RhaiConfig {
    /// Maximum number of operations a script can perform
    #[serde(default = "default_max_operations")]
    pub max_operations: u64,
    
    /// Maximum function call nesting depth
    #[serde(default = "default_max_call_levels")]
    pub max_call_levels: usize,
    
    /// Maximum size of strings in characters
    #[serde(default = "default_max_string_size")]
    pub max_string_size: usize,
    
    /// Maximum number of array elements
    #[serde(default = "default_max_array_size")]
    pub max_array_size: usize,
    
    /// Maximum execution time per script
    #[serde(default = "default_execution_timeout", deserialize_with = "deserialize_duration_from_millis")]
    pub execution_timeout: Duration,
}

impl Default for RhaiConfig {
    fn default() -> Self {
        Self {
            max_operations: default_max_operations(),
            max_call_levels: default_max_call_levels(),
            max_string_size: default_max_string_size(),
            max_array_size: default_max_array_size(),
            execution_timeout: default_execution_timeout(),
        }
    }
}

/// Provides the default value for shutdown_timeout_secs.
fn default_shutdown_timeout() -> u64 {
    30 // Default to 30 seconds
}

/// Default security configuration values
fn default_max_operations() -> u64 {
    100_000
}

fn default_max_call_levels() -> usize {
    10
}

fn default_max_string_size() -> usize {
    8_192
}

fn default_max_array_size() -> usize {
    1_000
}

fn default_execution_timeout_ms() -> u64 {
    5_000
}

/// Default value for execution timeout
fn default_execution_timeout() -> Duration {
    Duration::from_millis(default_execution_timeout_ms())
}

/// Custom deserializer for Duration from milliseconds
fn deserialize_duration_from_millis<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    let millis = u64::deserialize(deserializer)?;
    Ok(Duration::from_millis(millis))
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
            block_chunk_size: 10
            polling_interval_ms: 1000
            confirmation_blocks: 12
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
            block_chunk_size: 10
            polling_interval_ms: 1000
            confirmation_blocks: 12
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
        assert_eq!(app_config.shutdown_timeout_secs, 30);
    }

    #[test]
    fn test_config_with_block_chunk_size() {
        let yaml = "
            database_url: 'sqlite:test.db'
            rpc_urls: ['http://localhost:8545']
            network_id: 'testnet'
            block_chunk_size: 11
            polling_interval_ms: 1000
            confirmation_blocks: 12
        ";

        let builder =
            Config::builder().add_source(config::File::from_str(yaml, config::FileFormat::Yaml));
        let app_config: AppConfig = builder.build().unwrap().try_deserialize().unwrap();

        assert_eq!(app_config.block_chunk_size, 11);
    }

    #[test]
    fn test_config_with_polling_interval() {
        let yaml = "
            database_url: 'sqlite:test.db'
            rpc_urls: ['http://localhost:8545']
            network_id: 'testnet'
            polling_interval_ms: 5000
            block_chunk_size: 10
            confirmation_blocks: 12
        ";
        let builder =
            Config::builder().add_source(config::File::from_str(yaml, config::FileFormat::Yaml));
        let app_config: AppConfig = builder.build().unwrap().try_deserialize().unwrap();

        assert_eq!(app_config.polling_interval_ms, 5000);
    }

    #[test]
    fn test_config_with_shutdown_timeout() {
        let yaml = "
            database_url: 'sqlite:test.db'
            rpc_urls: ['http://localhost:8545']
            network_id: 'testnet'
            block_chunk_size: 10
            polling_interval_ms: 1000
            confirmation_blocks: 12
            shutdown_timeout_secs: 60
        ";
        let builder =
            Config::builder().add_source(config::File::from_str(yaml, config::FileFormat::Yaml));
        let app_config: AppConfig = builder.build().unwrap().try_deserialize().unwrap();

        assert_eq!(app_config.shutdown_timeout_secs, 60);
    }
}
