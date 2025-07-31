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

    /// Fetches the current block number from the data source.
    async fn get_current_block_number(&self) -> Result<u64, DataSourceError>;
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
    #[tracing::instrument(skip(provider), level = "debug")]
    pub fn new(provider: P) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl<P> DataSource for EvmRpcSource<P>
where
    P: Provider + Send + Sync,
{
    #[tracing::instrument(skip(self), level = "debug")]
    async fn fetch_logs(
        &self,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<primitives::Log>, DataSourceError> {
        tracing::debug!(from_block, to_block, "Fetching logs from RPC.");
        let filter = Filter::new().from_block(from_block).to_block(to_block);
        let logs: Vec<Log> = self.provider.get_logs(&filter).await.map_err(|e| {
            tracing::error!(error = %e, from_block, to_block, "Failed to fetch logs from RPC.");
            DataSourceError::Provider(Box::new(e))
        })?;
        tracing::debug!(
            log_count = logs.len(),
            from_block,
            to_block,
            "Successfully fetched logs."
        );
        // Convert from RPC log type to primitive log type
        let primitive_logs = logs.into_iter().map(|log| log.into()).collect();
        Ok(primitive_logs)
    }

    #[tracing::instrument(skip(self), level = "debug")]
    async fn get_current_block_number(&self) -> Result<u64, DataSourceError> {
        tracing::debug!("Fetching current block number from RPC.");
        let block_number = self.provider.get_block_number().await.map_err(|e| {
            tracing::error!(error = %e, "Failed to fetch current block number from RPC.");
            DataSourceError::Provider(Box::new(e))
        })?;
        tracing::debug!(
            current_block = block_number,
            "Successfully fetched current block number."
        );
        Ok(block_number)
    }
}

/// Creates a new `EvmRpcSource` with an HTTP provider.
#[tracing::instrument(level = "debug")]
pub fn new_http_source(rpc_url: &str) -> Result<EvmRpcSource<impl Provider>, DataSourceError> {
    tracing::debug!(rpc_url, "Creating new HTTP data source.");
    let url = Url::parse(rpc_url)?;
    let provider = ProviderBuilder::new().connect_http(url);
    tracing::info!(rpc_url, "HTTP data source created.");
    Ok(EvmRpcSource::new(provider))
}
