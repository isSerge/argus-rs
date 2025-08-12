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

/// Default value for execution timeout
fn default_execution_timeout() -> Duration {
    Duration::from_millis(5_000)
}

/// Custom deserializer for Duration from milliseconds
fn deserialize_duration_from_millis<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    let millis = u64::deserialize(deserializer)?;
    Ok(Duration::from_millis(millis))
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::Config;

    #[test]
    fn test_rhai_config_default() {
        let config = RhaiConfig::default();
        assert_eq!(config.max_operations, 100_000);
        assert_eq!(config.max_call_levels, 10);
        assert_eq!(config.max_string_size, 8_192);
        assert_eq!(config.max_array_size, 1_000);
        assert_eq!(config.execution_timeout, Duration::from_millis(5_000));
    }

    #[test]
    fn test_rhai_config_custom_values_yaml() {
        let yaml = "
            max_operations: 50000
            max_call_levels: 5
            max_string_size: 4096
            max_array_size: 500
            execution_timeout: 3000
        ";

        let builder =
            Config::builder().add_source(config::File::from_str(yaml, config::FileFormat::Yaml));
        let config: RhaiConfig = builder.build().unwrap().try_deserialize().unwrap();

        assert_eq!(config.max_operations, 50_000);
        assert_eq!(config.max_call_levels, 5);
        assert_eq!(config.max_string_size, 4_096);
        assert_eq!(config.max_array_size, 500);
        assert_eq!(config.execution_timeout, Duration::from_millis(3_000));
    }

    #[test]
    fn test_rhai_config_partial_yaml_uses_defaults() {
        let yaml = "
            max_operations: 75000
            execution_timeout: 7500
        ";

        let builder =
            Config::builder().add_source(config::File::from_str(yaml, config::FileFormat::Yaml));
        let config: RhaiConfig = builder.build().unwrap().try_deserialize().unwrap();

        assert_eq!(config.max_operations, 75_000);
        assert_eq!(config.max_call_levels, default_max_call_levels()); // Should use default
        assert_eq!(config.max_string_size, default_max_string_size()); // Should use default
        assert_eq!(config.max_array_size, default_max_array_size()); // Should use default
        assert_eq!(config.execution_timeout, Duration::from_millis(7_500));
    }

    #[test]
    fn test_deserialize_duration_from_millis() {
        let yaml = "execution_timeout: 1234";
        #[derive(Deserialize)]
        struct TestConfig {
            #[serde(deserialize_with = "super::deserialize_duration_from_millis")]
            execution_timeout: Duration,
        }
        let builder = Config::builder().add_source(config::File::from_str(yaml, config::FileFormat::Yaml));
        let config: TestConfig = builder.build().unwrap().try_deserialize().unwrap();
        assert_eq!(config.execution_timeout, Duration::from_millis(1234));
    }
}
