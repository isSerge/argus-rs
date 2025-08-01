//! This module defines the interface for fetching data from an EVM-compatible blockchain.

use crate::block_fetcher::{BlockFetcher, BlockFetcherError};
use alloy::{
    providers::Provider,
    rpc::types::{Block, Log},
};
use async_trait::async_trait;
use thiserror::Error;

/// Custom error type for data source operations.
#[derive(Error, Debug)]
pub enum DataSourceError {
    /// Error when parsing the RPC URL.
    #[error("Failed to parse RPC URL: {0}")]
    UrlParse(#[from] url::ParseError),

    /// Error when interacting with the provider.
    #[error("Provider error: {0}")]
    Provider(#[from] Box<dyn std::error::Error + Send + Sync>),

    /// Error originating from the `BlockFetcher`.
    #[error("Block fetcher error: {0}")]
    BlockFetcher(#[from] BlockFetcherError),

    /// Indicates that the requested block was not found.
    #[error("Block not found: {0}")]
    BlockNotFound(u64),
}

/// A trait for a data source that can fetch blockchain data.
#[async_trait]
pub trait DataSource {
    /// Fetches the core data for a single block (block with transactions and logs).
    async fn fetch_block_core_data(
        &self,
        block_number: u64,
    ) -> Result<(Block, Vec<Log>), DataSourceError>;

    /// Fetches the current block number from the data source.
    async fn get_current_block_number(&self) -> Result<u64, DataSourceError>;
}

/// A `DataSource` implementation that fetches data from an EVM RPC endpoint.
pub struct EvmRpcSource<P> {
    block_fetcher: BlockFetcher<P>,
}

impl<P> EvmRpcSource<P>
where
    P: Provider,
{
    /// Creates a new `EvmRpcSource`.
    #[tracing::instrument(skip(provider), level = "debug")]
    pub fn new(provider: P) -> Self {
        Self {
            block_fetcher: BlockFetcher::new(provider),
        }
    }
}

#[async_trait]
impl<P> DataSource for EvmRpcSource<P>
where
    P: Provider + Send + Sync,
{
    #[tracing::instrument(skip(self), level = "debug")]
    async fn fetch_block_core_data(
        &self,
        block_number: u64,
    ) -> Result<(Block, Vec<Log>), DataSourceError> {
        tracing::debug!(block_number, "Fetching core block data.");
        match self.block_fetcher.fetch_block_and_logs(block_number).await {
            Ok(data) => {
                tracing::debug!(block_number, "Successfully fetched core block data.");
                Ok(data)
            }
            Err(BlockFetcherError::BlockNotFound(num)) => {
                tracing::warn!(block_number = num, "Block not found.");
                Err(DataSourceError::BlockNotFound(num))
            }
            Err(e) => {
                tracing::error!(error = %e, block_number, "Failed to fetch core block data.");
                Err(DataSourceError::BlockFetcher(e))
            }
        }
    }

    #[tracing::instrument(skip(self), level = "debug")]
    async fn get_current_block_number(&self) -> Result<u64, DataSourceError> {
        tracing::debug!("Fetching current block number from RPC.");
        let block_number = self
            .block_fetcher
            .get_current_block_number()
            .await
            .map_err(Into::<DataSourceError>::into)?;
        tracing::debug!(
            current_block = block_number,
            "Successfully fetched current block number."
        );
        Ok(block_number)
    }
}
