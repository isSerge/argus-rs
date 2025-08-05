use serde::{Deserialize, Deserializer};
use std::time::Duration;

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
    #[serde(
        default = "default_execution_timeout",
        deserialize_with = "deserialize_duration_from_millis"
    )]
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
