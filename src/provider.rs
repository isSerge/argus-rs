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

use crate::config::RetryConfig;

/// Creates a new provider with the given RPC URLs.
pub fn create_provider(
    urls: Vec<Url>,
    retry_config: RetryConfig,
) -> Result<impl Provider, ProviderError> {
    if urls.is_empty() {
        return Err(ProviderError::CreationError(
            "RPC URL list cannot be empty".into(),
        ));
    }

    // Create a FallbackLayer with the provided URLs
    let fallback_layer = FallbackLayer::default()
        .with_active_transport_count(NonZeroUsize::new(urls.len()).unwrap());

    let transports: Vec<_> = urls.into_iter().map(Http::new).collect();

    // Instantiate the RetryBackoffLayer with the configuration
    let retry_layer = RetryBackoffLayer::new(
        retry_config.max_retry,
        retry_config.backoff_ms,
        retry_config.compute_units_per_second,
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
