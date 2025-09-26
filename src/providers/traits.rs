//! This module defines the interface for fetching data from an EVM-compatible
//! blockchain.

use std::collections::HashMap;

use alloy::{
    primitives::TxHash,
    rpc::types::{Block, Log, TransactionReceipt},
};
use async_trait::async_trait;
#[cfg(test)]
use mockall::automock;
use thiserror::Error;

use super::block_fetcher::BlockFetcherError;
use crate::persistence::error::PersistenceError;

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

    /// The channel for communicating with a downstream service was closed
    /// unexpectedly.
    #[error("Channel closed")]
    ChannelClosed,

    /// An error occurred while interacting with the state repository.
    #[error("State repository error: {0}")]
    StateRepository(#[from] PersistenceError),
}

/// A trait for a data source that can fetch blockchain data.
#[cfg_attr(test, automock)]
#[async_trait]
pub trait DataSource: Send + Sync {
    /// Fetches the core data for a single block (block with transactions and
    /// logs).
    async fn fetch_block_core_data(
        &self,
        block_number: u64,
    ) -> Result<(Block, Vec<Log>), DataSourceError>;

    /// Fetches the current block number from the data source.
    async fn get_current_block_number(&self) -> Result<u64, DataSourceError>;

    /// Fetches transaction receipts for the given transaction hashes.
    async fn fetch_receipts(
        &self,
        tx_hashes: &[TxHash],
    ) -> Result<HashMap<TxHash, TransactionReceipt>, DataSourceError>;
}
