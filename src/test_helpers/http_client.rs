use reqwest::Client;
use reqwest_middleware::ClientWithMiddleware;
use std::sync::Arc;

use crate::{
    config::HttpRetryConfig,
    http_client::{HttpClientPool, create_retryable_http_client},
};

/// Creates a default HTTP client with retry capabilities for testing purposes.
pub fn create_test_http_client() -> Arc<ClientWithMiddleware> {
    let retryable_client = create_retryable_http_client(&HttpRetryConfig::default(), Client::new());

    Arc::new(retryable_client)
}

/// Creates a test HTTP client from the http client pool.
/// Currently used for integration tests
pub async fn get_http_client_from_http_pool() -> Arc<ClientWithMiddleware> {
    let pool = HttpClientPool::new();
    let retry_policy = HttpRetryConfig::default();
    let http_client = pool.get_or_create(&retry_policy).await;
    http_client.unwrap()
}
