//! This module provides a retryable HTTP client and a pool for managing multiple clients.

mod client;
mod pool;

pub use client::create_retryable_http_client;
pub use pool::{HttpClientPool, HttpClientPoolError};
