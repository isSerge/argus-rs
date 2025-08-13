use super::{
    deserialize_duration_from_ms, deserialize_duration_from_seconds, serialize_duration_to_ms,
    serialize_duration_to_seconds,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

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
        deserialize_with = "deserialize_duration_from_ms",
        serialize_with = "serialize_duration_to_ms"
    )]
    pub initial_backoff_ms: Duration,
    /// Maximum backoff duration for retries
    #[serde(
        default = "default_max_backoff_ms",
        deserialize_with = "deserialize_duration_from_seconds",
        serialize_with = "serialize_duration_to_seconds"
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

#[cfg(test)]
mod tests {
    use super::*;
    use config::Config;
    use std::time::Duration;

    #[test]
    fn test_http_retry_config_with_custom_values() {
        let yaml = "
            max_retries: 5
            base_for_backoff: 100
            initial_backoff_ms: 200
            max_backoff_secs: 10
            jitter: full
        ";

        let builder =
            Config::builder().add_source(config::File::from_str(yaml, config::FileFormat::Yaml));
        let config: HttpRetryConfig = builder.build().unwrap().try_deserialize().unwrap();

        assert_eq!(config.max_retries, 5);
        assert_eq!(config.base_for_backoff, 100);
        assert_eq!(config.initial_backoff_ms, Duration::from_millis(200));
        assert_eq!(config.max_backoff_secs, Duration::from_secs(10));
        assert_eq!(config.jitter, JitterSetting::Full);
    }

    #[test]
    fn test_http_retry_config_without_custom_values_uses_default() {
        let default_config = HttpRetryConfig::default();
        let yaml = ""; // Empty YAML, so defaults should be used

        let builder =
            Config::builder().add_source(config::File::from_str(yaml, config::FileFormat::Yaml));
        let config: HttpRetryConfig = builder.build().unwrap().try_deserialize().unwrap();

        assert_eq!(config.max_retries, default_config.max_retries);
        assert_eq!(config.base_for_backoff, default_config.base_for_backoff);
        assert_eq!(config.initial_backoff_ms, default_config.initial_backoff_ms);
        assert_eq!(config.max_backoff_secs, default_config.max_backoff_secs);
        assert_eq!(config.jitter, default_config.jitter);
    }

    #[test]
    fn test_deserialize_duration_from_ms() {
        let yaml = "initial_backoff_ms: 1234";
        #[derive(Deserialize)]
        struct TestConfig {
            #[serde(deserialize_with = "deserialize_duration_from_ms")]
            initial_backoff_ms: Duration,
        }
        let builder =
            Config::builder().add_source(config::File::from_str(yaml, config::FileFormat::Yaml));
        let config: TestConfig = builder.build().unwrap().try_deserialize().unwrap();
        assert_eq!(config.initial_backoff_ms, Duration::from_millis(1234));
    }

    #[test]
    fn test_deserialize_duration_from_seconds() {
        let yaml = "max_backoff_secs: 5";
        #[derive(Deserialize)]
        struct TestConfig {
            #[serde(deserialize_with = "deserialize_duration_from_seconds")]
            max_backoff_secs: Duration,
        }
        let builder =
            Config::builder().add_source(config::File::from_str(yaml, config::FileFormat::Yaml));
        let config: TestConfig = builder.build().unwrap().try_deserialize().unwrap();
        assert_eq!(config.max_backoff_secs, Duration::from_secs(5));
    }
}
