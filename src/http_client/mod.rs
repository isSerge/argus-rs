//! This module provides functionality to create a retryable HTTP client with middleware
//! for handling transient errors, such as network issues or rate limiting.

use crate::config::{HttpRetryConfig, JitterSetting};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{
    Jitter, RetryTransientMiddleware, RetryableStrategy, policies::ExponentialBackoff,
};

/// Creates a retryable HTTP client with middleware for a single URL
///
/// # Parameters:
/// - `config`: Configuration for retry policies
/// - `base_client`: The base HTTP client to use
/// - `custom_strategy`: Optional custom retry strategy, complementing the default retry behavior
///
/// # Returns
/// A `ClientWithMiddleware` that includes retry capabilities
///
pub fn create_retryable_http_client<S>(
    config: &HttpRetryConfig,
    base_client: reqwest::Client,
    custom_strategy: Option<S>,
) -> ClientWithMiddleware
where
    S: RetryableStrategy + Send + Sync + 'static,
{
    // Determine the jitter setting and create the policy builder accordingly
    let policy_builder = match config.jitter {
        JitterSetting::None => ExponentialBackoff::builder().jitter(Jitter::None),
        JitterSetting::Full => ExponentialBackoff::builder().jitter(Jitter::Full),
    };

    // Create the retry policy based on the provided configuration
    let retry_policy = policy_builder
        .base(config.base_for_backoff)
        .retry_bounds(config.initial_backoff_ms, config.max_backoff_secs)
        .build_with_max_retries(config.max_retries);

    // If a custom strategy is provided, use it with the retry policy; otherwise, use the retry policy with the default strategy.
    if let Some(strategy) = custom_strategy {
        ClientBuilder::new(base_client).with(
            RetryTransientMiddleware::new_with_policy_and_strategy(retry_policy, strategy),
        )
    } else {
        ClientBuilder::new(base_client)
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
    }
    .build()
}
