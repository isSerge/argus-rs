use serde::{Deserialize, Deserializer, Serialize};
use std::time::Duration;

// --- Custom deserializer for Duration from milliseconds ---
fn deserialize_duration_from_ms<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    let ms = u64::deserialize(deserializer)?;
    Ok(Duration::from_millis(ms))
}

// --- Custom deserializer for Duration from seconds ---
fn deserialize_duration_from_seconds<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    let secs = u64::deserialize(deserializer)?;
    Ok(Duration::from_secs(secs))
}

/// --- Default values for retry configuration settings ---
fn default_max_attempts() -> u32 {
    3
}

fn default_initial_backoff_ms() -> Duration {
    Duration::from_millis(250)
}

fn default_max_backoff_ms() -> Duration {
    Duration::from_millis(10_000)
}

fn default_base_for_backoff() -> u32 {
    2
}

/// Serializable setting for jitter in retry policies
#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum JitterSetting {
    /// No jitter applied to the backoff duration
    None,
    /// Full jitter applied, randomizing the backoff duration
    #[default]
    Full,
}

/// Configuration for HTTP (RPC and Webhook notifiers) and SMTP (Email notifier) retry policies
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct HttpRetryConfig {
    /// Maximum number of retries for transient errors
    #[serde(default = "default_max_attempts")]
    pub max_retries: u32,
    /// Base duration for exponential backoff calculations
    #[serde(default = "default_base_for_backoff")]
    pub base_for_backoff: u32,
    /// Initial backoff duration before the first retry
    #[serde(
        default = "default_initial_backoff_ms",
        deserialize_with = "deserialize_duration_from_ms"
    )]
    pub initial_backoff_ms: Duration,
    /// Maximum backoff duration for retries
    #[serde(
        default = "default_max_backoff_ms",
        deserialize_with = "deserialize_duration_from_seconds"
    )]
    pub max_backoff_secs: Duration,
    /// Jitter to apply to the backoff duration
    #[serde(default)]
    pub jitter: JitterSetting,
}

impl Default for HttpRetryConfig {
    /// Creates a default configuration with reasonable retry settings
    fn default() -> Self {
        Self {
            max_retries: default_max_attempts(),
            base_for_backoff: default_base_for_backoff(),
            initial_backoff_ms: default_initial_backoff_ms(),
            max_backoff_secs: default_max_backoff_ms(),
            jitter: JitterSetting::default(),
        }
    }
}
