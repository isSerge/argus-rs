use super::{
    HttpRetryConfig, RhaiConfig, RpcRetryConfig, deserialize_duration_from_ms,
    deserialize_duration_from_seconds, deserialize_urls,
};
use config::{Config, ConfigError, File};
use serde::Deserialize;
use std::time::Duration;
use url::Url;

/// Provides the default value for shutdown_timeout_secs.
fn default_shutdown_timeout() -> Duration {
    Duration::from_secs(30)
}

/// Provides the default value for notification_channel_capacity.
fn default_notification_channel_capacity() -> u32 {
    1024
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
    pub monitor_config_path: String,

    /// Path to trigger configuration file.
    pub trigger_config_path: String,

    /// Optional retry configuration.
    #[serde(default)]
    pub rpc_retry_config: RpcRetryConfig,

    /// Configuration for HTTP client retry policies.
    #[serde(default)]
    pub http_retry_config: HttpRetryConfig,

    /// The size of the block chunk to process at once.
    pub block_chunk_size: u64,

    /// The interval in milliseconds to poll for new blocks.
    #[serde(
        deserialize_with = "deserialize_duration_from_ms",
        serialize_with = "serialize_duration_to_ms"
    )]
    pub polling_interval: Duration,

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
}

impl AppConfig {
    /// Creates a new `AppConfig` by reading from the configuration file.
    pub fn new(config_path: Option<&str>) -> Result<Self, ConfigError> {
        let s = Config::builder()
            .add_source(File::with_name(config_path.unwrap_or("config.yaml")))
            .build()?;
        s.try_deserialize()
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
        self.config.monitor_config_path = path.to_string();
        self
    }

    pub fn trigger_config_path(mut self, path: &str) -> Self {
        self.config.trigger_config_path = path.to_string();
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
}
