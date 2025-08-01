//! This module provides functionality to create a provider for EVM RPC requests
//! with retry logic and backoff strategies.

use std::num::NonZeroUsize;

use alloy::{
    providers::{Provider, ProviderBuilder},
    rpc::client::RpcClient,
    transports::{
        http::{Http, reqwest::Url},
        layers::{FallbackLayer, RetryBackoffLayer},
    },
};
use tower::ServiceBuilder;

/// Custom error type for provider operations.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// Error when creating the provider.
    #[error("Provider creation failed: {0}")]
    CreationError(String),
}

/// A provider configuration for retrying requests with backoff.
#[derive(Debug, Clone)]
pub struct RetryBackoff {
    /// The maximum number of retries for a request.
    pub max_retry: u32,
    /// The initial backoff delay in milliseconds.
    pub backoff_ms: u64,
    /// The number of compute units per second to allow.
    pub compute_units_per_second: u64,
}

impl RetryBackoff {
    /// Creates a new `RetryBackoff` configuration.
    pub fn new(max_retry: u32, backoff_ms: u64, compute_units_per_second: u64) -> Self {
        Self {
            max_retry,
            backoff_ms,
            compute_units_per_second,
        }
    }
}

impl Default for RetryBackoff {
    fn default() -> Self {
        Self {
            max_retry: 10,
            backoff_ms: 1000,
            compute_units_per_second: 100,
        }
    }
}

/// Creates a new provider with the given RPC URLs.
pub fn create_provider(
    urls: Vec<Url>,
    retry_backoff: RetryBackoff,
) -> Result<impl Provider, ProviderError> {
    if urls.is_empty() {
        return Err(ProviderError::CreationError(
            "RPC URL list cannot be empty".into(),
        ));
    }

    // Create a FallbackLayer with the provided URLs
    let fallback_layer = FallbackLayer::default()
        .with_active_transport_count(NonZeroUsize::new(urls.len()).unwrap());

    let transports: Vec<_> = urls.into_iter().map(|url| Http::new(url)).collect();

    // Instantiate the RetryBackoffLayer with the configuration
    let retry_layer = RetryBackoffLayer::new(
        retry_backoff.max_retry,
        retry_backoff.backoff_ms,
        retry_backoff.compute_units_per_second,
    );

    // Apply the layers
    let service = ServiceBuilder::new()
        .layer(retry_layer)
        .layer(fallback_layer)
        .service(transports);

    let client = RpcClient::builder().transport(service, false);
    let provider = ProviderBuilder::new().connect_client(client);
    Ok(provider)
}
