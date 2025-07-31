//! This module defines the interface for fetching data from an EVM-compatible blockchain.

use alloy::{
    primitives,
    providers::{Provider, ProviderBuilder},
    rpc::types::{Filter, Log},
};
use async_trait::async_trait;
use thiserror::Error;
use url::Url;

/// Custom error type for data source operations.
#[derive(Error, Debug)]
pub enum DataSourceError {
    /// Error when parsing the RPC URL.
    #[error("Failed to parse RPC URL: {0}")]
    UrlParse(#[from] url::ParseError),

    /// Error when interacting with the provider.
    #[error("Provider error: {0}")]
    Provider(#[from] Box<dyn std::error::Error + Send + Sync>),
}

/// A trait for a data source that can fetch blockchain data.
#[async_trait]
pub trait DataSource {
    /// Fetches logs from the data source within a given block range.
    async fn fetch_logs(
        &self,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<primitives::Log>, DataSourceError>;
}

/// A `DataSource` implementation that fetches data from an EVM RPC endpoint.
pub struct EvmRpcSource<P> {
    provider: P,
}

impl<P> EvmRpcSource<P>
where
    P: Provider,
{
    /// Creates a new `EvmRpcSource`.
    pub fn new(provider: P) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl<P> DataSource for EvmRpcSource<P>
where
    P: Provider + Send + Sync,
{
    async fn fetch_logs(
        &self,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<primitives::Log>, DataSourceError> {
        let filter = Filter::new().from_block(from_block).to_block(to_block);
        let logs: Vec<Log> = self
            .provider
            .get_logs(&filter)
            .await
            .map_err(|e| DataSourceError::Provider(Box::new(e)))?;
        // Convert from RPC log type to primitive log type
        let primitive_logs = logs.into_iter().map(|log| log.into()).collect();
        Ok(primitive_logs)
    }
}

/// Creates a new `EvmRpcSource` with an HTTP provider.
pub fn new_http_source(rpc_url: &str) -> Result<EvmRpcSource<impl Provider>, DataSourceError> {
    let url = Url::parse(rpc_url)?;
    let provider = ProviderBuilder::new().connect_http(url);
    Ok(EvmRpcSource::new(provider))
}
