use serde::Deserialize;

/// Configuration for the retry backoff policy.
#[derive(Debug, Deserialize, Clone)]
pub struct RetryConfig {
    /// The maximum number of retries for a request.
    pub max_retry: u32,
    /// The initial backoff delay in milliseconds.
    pub backoff_ms: u64,
    /// The number of compute units per second to allow.
    pub compute_units_per_second: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retry: 10,
            backoff_ms: 1000,
            compute_units_per_second: 100,
        }
    }
}
