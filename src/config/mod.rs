//! Configuration module for Argus.

mod monitor_loader;
mod retry;
mod rhai;

use config::{Config, ConfigError, File};
pub use monitor_loader::{MonitorLoader, MonitorLoaderError};
pub use retry::RetryConfig;
pub use rhai::RhaiConfig;
use serde::{Deserialize, Deserializer};
use url::Url;

/// Provides the default value for shutdown_timeout_secs.
fn default_shutdown_timeout() -> u64 {
    30 // Default to 30 seconds
}

/// Application configuration for Argus.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct AppConfig {
    /// Database URL for the SQLite database.
    pub database_url: String,

    /// RPC URLs for the Ethereum node.
    #[serde(deserialize_with = "deserialize_urls")]
    pub rpc_urls: Vec<Url>,

    /// Network ID for the Ethereum network.
    pub network_id: String,

    /// Optional path to monitor configuration file.
    #[serde(default)]
    pub monitor_config_path: Option<String>,

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

    #[test]
    fn test_config_with_rhai_config() {
        let yaml = "
            database_url: 'sqlite:test.db'
            rpc_urls: ['http://localhost:8545']
            network_id: 'testnet'
            block_chunk_size: 10
            polling_interval_ms: 1000
            confirmation_blocks: 12
            rhai:
              max_operations: 250000
              max_call_levels: 15
              max_string_size: 16384
              max_array_size: 2000
              execution_timeout: 7500
        ";

        let builder =
            Config::builder().add_source(config::File::from_str(yaml, config::FileFormat::Yaml));
        let app_config: AppConfig = builder.build().unwrap().try_deserialize().unwrap();

        assert_eq!(app_config.rhai.max_operations, 250_000);
        assert_eq!(app_config.rhai.max_call_levels, 15);
        assert_eq!(app_config.rhai.max_string_size, 16_384);
        assert_eq!(app_config.rhai.max_array_size, 2_000);
        assert_eq!(
            app_config.rhai.execution_timeout,
            std::time::Duration::from_millis(7_500)
        );
    }

    #[test]
    fn test_config_without_rhai_uses_defaults() {
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

        let default_rhai_config = RhaiConfig::default();
        assert_eq!(
            app_config.rhai.max_operations,
            default_rhai_config.max_operations
        );
        assert_eq!(
            app_config.rhai.max_call_levels,
            default_rhai_config.max_call_levels
        );
        assert_eq!(
            app_config.rhai.max_string_size,
            default_rhai_config.max_string_size
        );
        assert_eq!(
            app_config.rhai.max_array_size,
            default_rhai_config.max_array_size
        );
        assert_eq!(
            app_config.rhai.execution_timeout,
            default_rhai_config.execution_timeout
        );
    }

    #[test]
    fn test_config_with_partial_rhai_config() {
        let yaml = "
            database_url: 'sqlite:test.db'
            rpc_urls: ['http://localhost:8545']
            network_id: 'testnet'
            block_chunk_size: 10
            polling_interval_ms: 1000
            confirmation_blocks: 12
            rhai:
              max_operations: 500000
              execution_timeout: 10000
        ";

        let builder =
            Config::builder().add_source(config::File::from_str(yaml, config::FileFormat::Yaml));
        let app_config: AppConfig = builder.build().unwrap().try_deserialize().unwrap();

        // Specified values should be used
        assert_eq!(app_config.rhai.max_operations, 500_000);
        assert_eq!(
            app_config.rhai.execution_timeout,
            std::time::Duration::from_millis(10_000)
        );

        // Non-specified values should use defaults
        let default_rhai_config = RhaiConfig::default();
        assert_eq!(
            app_config.rhai.max_call_levels,
            default_rhai_config.max_call_levels
        );
        assert_eq!(
            app_config.rhai.max_string_size,
            default_rhai_config.max_string_size
        );
        assert_eq!(
            app_config.rhai.max_array_size,
            default_rhai_config.max_array_size
        );
    }

    #[test]
    fn test_config_with_invalid_rhai_values() {
        let yaml = "
            database_url: 'sqlite:test.db'
            rpc_urls: ['http://localhost:8545']
            network_id: 'testnet'
            block_chunk_size: 10
            polling_interval_ms: 1000
            confirmation_blocks: 12
            rhai:
              max_operations: 'invalid_number'
              execution_timeout: 5000
        ";

        let builder =
            Config::builder().add_source(config::File::from_str(yaml, config::FileFormat::Yaml));
        let result: Result<AppConfig, _> = builder.build().unwrap().try_deserialize();
        assert!(result.is_err()); // Should fail due to invalid max_operations value
    }
}
