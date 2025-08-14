//! A reusable, thread-safe pool for managing HTTP clients.
//!
//! This module provides a generic `HttpClientPool` that can be shared across the
//! application to create and reuse HTTP clients with different configurations.

use super::client::create_retryable_http_client;
use crate::config::HttpRetryConfig;
use reqwest::Client as ReqwestClient;
use reqwest_middleware::ClientWithMiddleware;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::RwLock;

/// Errors that can occur within the `HttpClientPool`.
#[derive(Debug, Error)]
pub enum HttpClientPoolError {
    /// An error occurred while building the underlying `reqwest::Client`.
    #[error("Failed to create HTTP client: {0}")]
    HttpClientBuildError(String),
}

/// A pool for managing and reusing HTTP clients for various services.
///
/// This struct provides a thread-safe way to create and access HTTP clients
/// with specific retry policies. A single instance of this pool can be shared
/// across the application. Services that need to make HTTP calls (e.g., a
/// webhook notifier, an Etherscan client) can request a client from the pool,
/// each with a specific `HttpRetryConfig`.
///
/// Clients are keyed by their `HttpRetryConfig` to ensure that different
/// retry strategies result in different, isolated clients.
pub struct HttpClientPool {
    clients: Arc<RwLock<HashMap<String, Arc<ClientWithMiddleware>>>>,
}

impl HttpClientPool {
    /// Creates a new, empty `HttpClientPool`.
    pub fn new() -> Self {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Gets an existing HTTP client from the pool or creates a new one if none exists
    /// for the given retry policy.
    ///
    /// This method ensures that only one client per `HttpRetryConfig` is created and
    /// reused, which is essential for connection pooling and performance. It uses a
    /// double-checked locking pattern to minimize contention.
    ///
    /// # Arguments
    /// * `retry_policy` - Configuration for the HTTP retry policy. This is used as the
    ///   unique key for the client in the pool.
    ///
    /// # Returns
    /// * `Result<Arc<ClientWithMiddleware>, HttpClientPoolError>` - The HTTP client
    ///   wrapped in an `Arc` for shared ownership, or an error if client creation fails.
    pub async fn get_or_create(
        &self,
        retry_policy: &HttpRetryConfig,
    ) -> Result<Arc<ClientWithMiddleware>, HttpClientPoolError> {
        let key = format!("{retry_policy:?}");

        // Fast path: Check if the client already exists with a read lock.
        if let Some(client) = self.clients.read().await.get(&key) {
            return Ok(client.clone());
        }

        // Slow path: If not found, acquire a write lock to create it.
        let mut clients = self.clients.write().await;
        // Double-check: Another thread might have created the client while we were
        // waiting for the write lock.
        if let Some(client) = clients.get(&key) {
            return Ok(client.clone());
        }

        // Create and insert the new client if it still doesn't exist.
        // TODO: make configurable
        let base_client = ReqwestClient::builder()
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(Some(Duration::from_secs(90)))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| HttpClientPoolError::HttpClientBuildError(e.to_string()))?;

        let new_client = Arc::new(create_retryable_http_client(retry_policy, base_client));
        clients.insert(key, new_client.clone());

        Ok(new_client)
    }

    /// Returns the number of active HTTP clients in the pool.
    #[cfg(test)]
    pub async fn get_active_client_count(&self) -> usize {
        self.clients.read().await.len()
    }
}

impl Default for HttpClientPool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_pool() -> HttpClientPool {
        HttpClientPool::new()
    }

    #[tokio::test]
    async fn test_pool_init_empty() {
        let pool = create_pool();
        let http_count = pool.get_active_client_count().await;

        assert_eq!(http_count, 0, "Pool should be empty initially");
    }

    #[tokio::test]
    async fn test_pool_get_or_create_http_client() {
        let pool = create_pool();
        let retry_config = HttpRetryConfig::default();
        let client = pool.get_or_create(&retry_config).await;

        assert!(
            client.is_ok(),
            "Should successfully create or get HTTP client"
        );

        assert_eq!(
            pool.get_active_client_count().await,
            1,
            "Pool should have one active HTTP client"
        );
    }

    #[tokio::test]
    async fn test_pool_returns_same_client() {
        let pool = create_pool();
        let retry_config = HttpRetryConfig::default();
        let client1 = pool.get_or_create(&retry_config).await.unwrap();
        let client2 = pool.get_or_create(&retry_config).await.unwrap();

        assert!(
            Arc::ptr_eq(&client1, &client2),
            "Should return the same client instance"
        );
        assert_eq!(
            pool.get_active_client_count().await,
            1,
            "Pool should still have one active HTTP client"
        );
    }

    #[tokio::test]
    async fn test_pool_concurrent_access() {
        let pool = Arc::new(create_pool());
        let retry_config = HttpRetryConfig::default();

        let num_tasks = 10;
        let mut tasks = Vec::new();

        for _ in 0..num_tasks {
            let pool_clone = Arc::clone(&pool);
            let retry_config = retry_config.clone();
            tasks.push(tokio::spawn(async move {
                let client = pool_clone.get_or_create(&retry_config).await;
                assert!(
                    client.is_ok(),
                    "Should successfully create or get HTTP client"
                );
            }));
        }

        let results = futures::future::join_all(tasks).await;

        for result in results {
            assert!(result.is_ok(), "All tasks should complete successfully");
        }
    }

    #[tokio::test]
    async fn test_pool_default() {
        let pool = HttpClientPool::default();
        let retry_config = HttpRetryConfig::default();

        assert_eq!(
            pool.get_active_client_count().await,
            0,
            "Default pool should be empty initially"
        );

        let client = pool.get_or_create(&retry_config).await;

        assert!(
            client.is_ok(),
            "Default pool should successfully create or get HTTP client"
        );

        assert_eq!(
            pool.get_active_client_count().await,
            1,
            "Default pool should have one active HTTP client"
        );
    }

    #[tokio::test]
    async fn test_pool_returns_different_http_clients_for_different_configs() {
        let pool = create_pool();

        // Config 1 (default)
        let retry_config_1 = HttpRetryConfig::default();

        // Config 2 (different retry count)
        let retry_config_2 = HttpRetryConfig {
            max_retries: 5,
            ..Default::default()
        };

        // Get a client for each config
        let client1 = pool.get_or_create(&retry_config_1).await.unwrap();
        let client2 = pool.get_or_create(&retry_config_2).await.unwrap();

        // Pointers should NOT be equal, as they are different clients
        assert!(
            !Arc::ptr_eq(&client1, &client2),
            "Should return different client instances for different configurations"
        );

        // The pool should now contain two distinct clients
        assert_eq!(
            pool.get_active_client_count().await,
            2,
            "Pool should have two active HTTP clients"
        );

        // Getting the first client again should return the original one
        let client1_again = pool.get_or_create(&retry_config_1).await.unwrap();
        assert!(
            Arc::ptr_eq(&client1, &client1_again),
            "Should return the same client instance when called again with the same config"
        );

        // Pool size should still be 2
        assert_eq!(
            pool.get_active_client_count().await,
            2,
            "Pool should still have two active HTTP clients after getting an existing one"
        );
    }
}
