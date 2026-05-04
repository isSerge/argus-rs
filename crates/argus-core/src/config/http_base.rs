use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{deserialize_duration_from_seconds, serialize_duration_to_seconds};

fn default_idle_per_host() -> usize {
    10
}

fn default_idle_timeout() -> Duration {
    Duration::from_secs(30)
}

fn default_connect_timeout() -> Duration {
    Duration::from_secs(10)
}

/// Configuration for the base HTTP client.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct BaseHttpClientConfig {
    /// Maximum idle connections per host
    #[serde(default = "default_idle_per_host")]
    pub max_idle_per_host: usize,

    /// Timeout for idle connections
    #[serde(
        default = "default_idle_timeout",
        deserialize_with = "deserialize_duration_from_seconds",
        serialize_with = "serialize_duration_to_seconds"
    )]
    pub idle_timeout: Duration,

    /// Timeout for establishing connections
    #[serde(
        default = "default_connect_timeout",
        deserialize_with = "deserialize_duration_from_seconds",
        serialize_with = "serialize_duration_to_seconds"
    )]
    pub connect_timeout: Duration,
}

impl Default for BaseHttpClientConfig {
    fn default() -> Self {
        Self {
            max_idle_per_host: default_idle_per_host(),
            idle_timeout: default_idle_timeout(),
            connect_timeout: default_connect_timeout(),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json;

    use super::*;

    #[test]
    fn test_base_http_client_config_default() {
        let config = BaseHttpClientConfig::default();
        assert_eq!(config.max_idle_per_host, 10);
        assert_eq!(config.idle_timeout, Duration::from_secs(30));
        assert_eq!(config.connect_timeout, Duration::from_secs(10));
    }

    #[test]
    fn test_base_http_client_config_custom_values_json() {
        let json = r#"{
            "max_idle_per_host": 20,
            "idle_timeout": 60,
            "connect_timeout": 5
        }"#;
        let config: BaseHttpClientConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.max_idle_per_host, 20);
        assert_eq!(config.idle_timeout, Duration::from_secs(60));
        assert_eq!(config.connect_timeout, Duration::from_secs(5));
    }

    #[test]
    fn test_base_http_client_config_partial_json_uses_defaults() {
        let json = r#"{
            "max_idle_per_host": 15
        }"#;
        let config: BaseHttpClientConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.max_idle_per_host, 15);
        assert_eq!(config.idle_timeout, Duration::from_secs(30)); // default
        assert_eq!(config.connect_timeout, Duration::from_secs(10)); // default
    }

    #[test]
    fn test_serialization_deserialization_roundtrip() {
        let config = BaseHttpClientConfig {
            max_idle_per_host: 25,
            idle_timeout: Duration::from_secs(120),
            connect_timeout: Duration::from_secs(15),
        };
        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized: BaseHttpClientConfig = serde_json::from_str(&serialized).unwrap();
        assert_eq!(config, deserialized);
    }
}
