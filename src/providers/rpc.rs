//! This module provides functionality to create a provider for EVM RPC requests
//! with retry logic and backoff strategies.

use std::{collections::HashMap, num::NonZeroUsize};

use alloy::{
    primitives::TxHash,
    providers::{Provider, ProviderBuilder, layers::CallBatchLayer},
    rpc::{
        client::RpcClient,
        types::{Block, Log, TransactionReceipt},
    },
    transports::{
        http::{Http, reqwest::Url},
        layers::{FallbackLayer, RetryBackoffLayer},
    },
};
use async_trait::async_trait;
use tower::ServiceBuilder;

use super::{
    block_fetcher::{BlockFetcher, BlockFetcherError},
    traits::{DataSource, DataSourceError},
};
use crate::config::RpcRetryConfig;

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
        Self { block_fetcher: BlockFetcher::new(provider) }
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
        tracing::debug!(current_block = block_number, "Successfully fetched current block number.");
        Ok(block_number)
    }

    #[tracing::instrument(skip(self), level = "debug")]
    async fn fetch_receipts(
        &self,
        tx_hashes: &[TxHash],
    ) -> Result<HashMap<TxHash, TransactionReceipt>, DataSourceError> {
        tracing::debug!(tx_count = tx_hashes.len(), "Fetching transaction receipts.");
        let receipts = self
            .block_fetcher
            .fetch_receipts(tx_hashes)
            .await
            .map_err(Into::<DataSourceError>::into)?;
        tracing::debug!(
            receipt_count = receipts.len(),
            "Successfully fetched transaction receipts."
        );
        Ok(receipts)
    }
}

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
    retry_config: RpcRetryConfig,
) -> Result<impl Provider, ProviderError> {
    if urls.is_empty() {
        return Err(ProviderError::CreationError("RPC URL list cannot be empty".into()));
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
    let service =
        ServiceBuilder::new().layer(retry_layer).layer(fallback_layer).service(transports);

    let client = RpcClient::builder().transport(service, false);
    let provider = ProviderBuilder::new().layer(CallBatchLayer::new()).connect_client(client);
    Ok(provider)
}
