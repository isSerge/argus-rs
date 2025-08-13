use serde::Deserialize;

/// Configuration for the RPC retry backoff policy.
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct RpcRetryConfig {
    /// The maximum number of retries for a request.
    pub max_retry: u32,
    /// The initial backoff delay in milliseconds.
    pub backoff_ms: u64, // Keep as u64 because alloy::transports::layers::RetryBackoffLayer expects it.
    /// The number of compute units per second to allow.
    pub compute_units_per_second: u64,
}

impl Default for RpcRetryConfig {
    fn default() -> Self {
        Self {
            max_retry: 10,
            backoff_ms: 1000,
            compute_units_per_second: 100,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::Config;

    #[test]
    fn test_rpc_retry_config_with_custom_values() {
        let yaml = "
            max_retry: 5
            backoff_ms: 500
            compute_units_per_second: 50
        ";

        let builder =
            Config::builder().add_source(config::File::from_str(yaml, config::FileFormat::Yaml));
        let config: RpcRetryConfig = builder.build().unwrap().try_deserialize().unwrap();

        assert_eq!(config.max_retry, 5);
        assert_eq!(config.backoff_ms, 500);
        assert_eq!(config.compute_units_per_second, 50);
    }

    #[test]
    fn test_rpc_retry_config_without_custom_values_uses_default() {
        let default_config = RpcRetryConfig::default();
        let yaml = ""; // Empty YAML, so defaults should be used

        let builder =
            Config::builder().add_source(config::File::from_str(yaml, config::FileFormat::Yaml));
        let config: RpcRetryConfig = builder.build().unwrap().try_deserialize().unwrap();

        assert_eq!(config.max_retry, default_config.max_retry);
        assert_eq!(config.backoff_ms, default_config.backoff_ms);
        assert_eq!(
            config.compute_units_per_second,
            default_config.compute_units_per_second
        );
    }
}
