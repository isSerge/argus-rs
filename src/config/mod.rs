//! Configuration module for Argus.

mod http_retry;
mod monitor_loader;
mod rhai;
mod rpc_retry;
mod trigger_loader;

use config::{Config, ConfigError, File};
pub use http_retry::{HttpRetryConfig, JitterSetting};
pub use monitor_loader::{MonitorLoader, MonitorLoaderError};
pub use rhai::RhaiConfig;
pub use rpc_retry::RpcRetryConfig;
use serde::{Deserialize, Deserializer};
pub use trigger_loader::{
    DiscordConfig, SlackConfig, TelegramConfig, TriggerConfig, TriggerLoader, TriggerLoaderError,
    TriggerTypeConfig, WebhookConfig,
};
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
