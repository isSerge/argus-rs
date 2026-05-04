use std::sync::Arc;

use argus_core::config::HttpRetryConfig;
use reqwest::Client;
use reqwest_middleware::ClientWithMiddleware;

use crate::http_client::create_retryable_http_client;

/// Creates a default HTTP client with retry capabilities for testing purposes.
pub fn create_test_http_client() -> Arc<ClientWithMiddleware> {
    let retryable_client = create_retryable_http_client(&HttpRetryConfig::default(), Client::new());

    Arc::new(retryable_client)
}
