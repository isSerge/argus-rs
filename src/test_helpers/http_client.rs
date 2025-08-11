use reqwest::Client;
use reqwest_middleware::ClientWithMiddleware;
use reqwest_retry::DefaultRetryableStrategy;
use std::sync::Arc;

use crate::{config::HttpRetryConfig, http_client::create_retryable_http_client};

/// Creates a default HTTP client with retry capabilities for testing purposes.
pub fn create_test_http_client() -> Arc<ClientWithMiddleware> {
    let retryable_client = create_retryable_http_client::<DefaultRetryableStrategy>(
        &HttpRetryConfig::default(),
        Client::new(),
        None,
    );

    Arc::new(retryable_client)
}
