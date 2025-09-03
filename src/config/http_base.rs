use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::{deserialize_duration_from_seconds, serialize_duration_to_seconds};

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
#[derive(Debug, Clone, Deserialize, Serialize)]
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
