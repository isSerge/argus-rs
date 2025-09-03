use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use config::{Config, ConfigError, File};
use serde::Deserialize;
use url::Url;

use super::{
    HttpRetryConfig, RhaiConfig, RpcRetryConfig, deserialize_duration_from_ms,
    deserialize_duration_from_seconds, deserialize_urls,
};
use crate::config::BaseHttpClientConfig;

/// Provides the default value for shutdown_timeout_secs.
fn default_shutdown_timeout() -> Duration {
    Duration::from_secs(30)
}

/// Provides the default value for notification_channel_capacity.
fn default_notification_channel_capacity() -> u32 {
    1024
}

/// Provides the default value for aggregation_check_interval_secs.
fn default_aggregation_check_interval_secs() -> Duration {
    Duration::from_secs(5)
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

    /// Path to monitor configuration file.
    #[serde(skip_deserializing)]
    pub monitor_config_path: PathBuf,

    /// Path to notifier configuration file.
    #[serde(skip_deserializing)]
    pub notifier_config_path: PathBuf,

    /// Optional retry configuration.
    #[serde(default)]
    pub rpc_retry_config: RpcRetryConfig,

    /// Configuration for HTTP client retry policies.
    #[serde(default)]
    pub http_retry_config: HttpRetryConfig,

    /// Configuration for the base HTTP client.
    #[serde(default)]
    pub http_base_config: BaseHttpClientConfig,

    /// The size of the block chunk to process at once.
    pub block_chunk_size: u64,

    /// The interval in milliseconds to poll for new blocks.
    #[serde(
        deserialize_with = "deserialize_duration_from_ms",
        serialize_with = "serialize_duration_to_ms"
    )]
    pub polling_interval_ms: Duration,

    /// Number of confirmation blocks to wait for before processing.
    pub confirmation_blocks: u64,

    /// The maximum time in seconds to wait for graceful shutdown.
    #[serde(
        deserialize_with = "deserialize_duration_from_seconds",
        serialize_with = "serialize_duration_to_seconds",
        default = "default_shutdown_timeout"
    )]
    pub shutdown_timeout: Duration,

    /// Rhai script execution configuration.
    #[serde(default)]
    pub rhai: RhaiConfig,

    /// The capacity of the channel used for sending notifications.
    #[serde(default = "default_notification_channel_capacity")]
    pub notification_channel_capacity: u32,

    /// Path to ABI configuration directory.
    pub abi_config_path: PathBuf,

    /// The interval in seconds to check for aggregated matches.
    #[serde(
        deserialize_with = "deserialize_duration_from_seconds",
        serialize_with = "serialize_duration_to_seconds",
        default = "default_aggregation_check_interval_secs"
    )]
    pub aggregation_check_interval_secs: Duration,
}

impl AppConfig {
    /// Creates a new `AppConfig` by reading from the configuration directory.
    pub fn new(config_dir: Option<&str>) -> Result<Self, ConfigError> {
        let config_dir_str = config_dir.unwrap_or("configs");
        let s = Config::builder()
            .add_source(File::with_name(&format!("{}/app.yaml", config_dir_str)))
            .build()?;
        let mut config: Self = s.try_deserialize()?;

        let config_path = Path::new(config_dir_str);

        // Join the config paths with the config directory
        // This ensures that the paths are correctly resolved relative to the config
        // directory.
        let monitor_path = config_path.join("monitors.yaml");
        config.monitor_config_path = monitor_path;

        let notifier_path = config_path.join("notifiers.yaml");
        config.notifier_config_path = notifier_path;

        Ok(config)
    }

    /// Creates a new `AppConfigBuilder` for testing purposes.
    #[cfg(test)]
    pub fn builder() -> AppConfigBuilder {
        AppConfigBuilder::default()
    }
}

/// A builder for creating `AppConfig` instances for testing.
#[cfg(test)]
#[derive(Default)]
pub struct AppConfigBuilder {
    config: AppConfig,
}

#[cfg(test)]
impl AppConfigBuilder {
    pub fn rpc_urls(mut self, rpc_urls: Vec<Url>) -> Self {
        self.config.rpc_urls = rpc_urls;
        self
    }

    pub fn network_id(mut self, network_id: &str) -> Self {
        self.config.network_id = network_id.to_string();
        self
    }

    pub fn monitor_config_path(mut self, path: &str) -> Self {
        self.config.monitor_config_path = path.into();
        self
    }

    pub fn notifier_config_path(mut self, path: &str) -> Self {
        self.config.notifier_config_path = path.into();
        self
    }

    pub fn database_url(mut self, url: &str) -> Self {
        self.config.database_url = url.to_string();
        self
    }

    pub fn confirmation_blocks(mut self, blocks: u64) -> Self {
        self.config.confirmation_blocks = blocks;
        self
    }

    pub fn build(self) -> AppConfig {
        self.config
    }

    pub fn abi_config_path(mut self, path: &str) -> Self {
        self.config.abi_config_path = path.into();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_config_builder() {
        let rpc_urls = vec![Url::parse("http://localhost:8545").unwrap()];
        let config = AppConfig::builder()
            .rpc_urls(rpc_urls)
            .network_id("testnet")
            .monitor_config_path("test_monitor.yaml")
            .notifier_config_path("test_notifier.yaml")
            .database_url("sqlite::memory:")
            .confirmation_blocks(12)
            .abi_config_path("abis/")
            .build();

        assert_eq!(config.rpc_urls.len(), 1);
        assert_eq!(config.network_id, "testnet");
        assert_eq!(config.monitor_config_path, PathBuf::from("test_monitor.yaml"));
        assert_eq!(config.notifier_config_path, PathBuf::from("test_notifier.yaml"));
        assert_eq!(config.database_url, "sqlite::memory:");
        assert_eq!(config.confirmation_blocks, 12);
    }

    #[test]
    fn test_app_config_from_file() {
        // Create a temporary config file for testing
        let config_content = r#"
        database_url: "sqlite::memory:"
        rpc_urls:
          - "http://localhost:8545"
        network_id: "testnet"
        confirmation_blocks: 12
        block_chunk_size: 0
        polling_interval_ms: 10000
        abi_config_path: abis/
        "#;
        let temp_dir = tempfile::tempdir().unwrap();
        let app_yaml_path = temp_dir.path().join("app.yaml");
        std::fs::write(&app_yaml_path, config_content).unwrap();

        let temp_dir_path = temp_dir.path();
        let config = AppConfig::new(Some(temp_dir_path.to_str().unwrap())).unwrap();
        assert!(!config.rpc_urls.is_empty());
        assert_eq!(config.network_id, "testnet");

        let expected_monitor_path = temp_dir_path.join("monitors.yaml");
        assert_eq!(config.monitor_config_path, PathBuf::from(expected_monitor_path));

        let expected_notifier_path = temp_dir_path.join("notifiers.yaml");
        assert_eq!(config.notifier_config_path, PathBuf::from(expected_notifier_path));

        assert_eq!(config.database_url, "sqlite::memory:");
        assert_eq!(config.confirmation_blocks, 12);
        assert_eq!(config.shutdown_timeout, Duration::from_secs(30));
        assert_eq!(config.notification_channel_capacity, 1024);
        assert_eq!(config.block_chunk_size, 0);
        assert_eq!(config.polling_interval_ms, Duration::from_millis(10000));
    }

    #[test]
    fn test_app_config_from_file_with_http_base_config() {
        let config_content = r#"
        database_url: "sqlite::memory:"
        rpc_urls:
          - "http://localhost:8545"
        network_id: "testnet"
        confirmation_blocks: 12
        block_chunk_size: 0
        polling_interval_ms: 10000
        abi_config_path: abis/
        http_base_config:
          max_idle_per_host: 50
          idle_timeout: 120
          connect_timeout: 20
        "#;
        let temp_dir = tempfile::tempdir().unwrap();
        let app_yaml_path = temp_dir.path().join("app.yaml");
        std::fs::write(&app_yaml_path, config_content).unwrap();

        let temp_dir_path = temp_dir.path();
        let config = AppConfig::new(Some(temp_dir_path.to_str().unwrap())).unwrap();

        assert_eq!(config.http_base_config.max_idle_per_host, 50);
        assert_eq!(config.http_base_config.idle_timeout, Duration::from_secs(120));
        assert_eq!(config.http_base_config.connect_timeout, Duration::from_secs(20));
    }
}
