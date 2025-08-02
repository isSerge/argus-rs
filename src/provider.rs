//! This module provides functionality to create a provider for EVM RPC requests
//! with retry logic and backoff strategies.

use crate::config::RetryConfig;
use alloy::{
    providers::{Provider, ProviderBuilder, layers::CallBatchLayer},
    rpc::client::RpcClient,
    transports::{
        http::{Http, reqwest::Url},
        layers::{FallbackLayer, RetryBackoffLayer},
    },
};
use std::num::NonZeroUsize;
use tower::ServiceBuilder;

/// Custom error type for provider operations.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// Error when creating the provider.
    #[error("Provider creation failed: {0}")]
    CreationError(String),
}

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
    let fallback_layer = FallbackLayer::default().with_active_transport_count(
        NonZeroUsize::new(urls.len()).expect("At least one URL is required"),
    );

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
    let provider = ProviderBuilder::new()
        .layer(CallBatchLayer::new())
        .connect_client(client);
    Ok(provider)
}
