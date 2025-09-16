//! This module provides functionality to create a provider for EVM RPC requests
//! with retry logic and backoff strategies.

use std::{collections::HashMap, num::NonZeroUsize, sync::Arc};

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
use crate::{config::RpcRetryConfig, monitor::MonitorManager};

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
    pub fn new(provider: P, monitor_manager: Arc<MonitorManager>) -> Self {
        Self { block_fetcher: BlockFetcher::new(provider, monitor_manager) }
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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use alloy::{
        network::Ethereum,
        primitives::{B256, Bloom, BloomInput, U256, address, b256},
        providers::{Provider, ProviderBuilder},
        transports::mock::Asserter,
    };

    use super::*;
    use crate::test_helpers::{
        BlockBuilder, LogBuilder, ReceiptBuilder, create_test_monitor_manager,
    };

    fn mock_provider() -> (impl Provider<Ethereum>, Asserter) {
        let asserter = Asserter::new();
        let provider = ProviderBuilder::new().connect_mocked_client(asserter.clone());
        (provider, asserter)
    }

    #[tokio::test]
    async fn test_fetch_block_core_data_success() {
        let (provider, asserter) = mock_provider();

        let monitored_address = address!("1111111111111111111111111111111111111111");

        // Create a block with a bloom filter that includes the monitored address
        let mut bloom = Bloom::default();
        bloom.accrue(BloomInput::Raw(monitored_address.as_slice()));

        let block = BlockBuilder::new().number(1).bloom(bloom).build();
        let log = LogBuilder::new().block_number(1).address(monitored_address).build();
        let alloy_log: Log = log.into();

        asserter.push_success(&block);
        asserter.push_success(&vec![alloy_log.clone()]);

        let monitor_manager = create_test_monitor_manager(vec![monitored_address], vec![]);
        let source = EvmRpcSource::new(provider, monitor_manager);

        let (fetched_block, fetched_logs) = source.fetch_block_core_data(1).await.unwrap();

        assert_eq!(fetched_block, block);
        assert_eq!(fetched_logs, vec![alloy_log]);
    }

    #[tokio::test]
    async fn test_fetch_block_core_data_block_not_found() {
        let (provider, asserter) = mock_provider();

        asserter.push_success(&Option::<Block>::None);
        asserter.push_success(&Vec::<Log>::new());

        let monitor_manager = create_test_monitor_manager(vec![], vec![]);
        let source = EvmRpcSource::new(provider, monitor_manager);

        let result = source.fetch_block_core_data(1).await;

        assert!(matches!(result, Err(DataSourceError::BlockNotFound(1))));
    }

    #[tokio::test]
    async fn test_fetch_block_core_data_error_handling() {
        let (provider, asserter) = mock_provider();
        asserter.push_failure_msg("RPC error");
        asserter.push_success(&Vec::<Log>::new());

        let monitor_manager = create_test_monitor_manager(vec![], vec![]);
        let source = EvmRpcSource::new(provider, monitor_manager);

        let result = source.fetch_block_core_data(1).await;

        assert!(matches!(result, Err(DataSourceError::BlockFetcher(_))));
    }

    #[tokio::test]
    async fn test_get_current_block_number() {
        let (provider, asserter) = mock_provider();
        asserter.push_success(&U256::from(1));

        let monitor_manager = create_test_monitor_manager(vec![], vec![]);
        let source = EvmRpcSource::new(provider, monitor_manager);

        let block_number = source.get_current_block_number().await.unwrap();

        assert_eq!(block_number, 1);
    }

    #[tokio::test]
    async fn test_get_current_block_number_error_handling() {
        let (provider, asserter) = mock_provider();
        asserter.push_failure_msg("RPC error");

        let monitor_manager = create_test_monitor_manager(vec![], vec![]);
        let source = EvmRpcSource::new(provider, monitor_manager);

        let result = source.get_current_block_number().await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_fetch_receipts_success() {
        let (provider, asserter) = mock_provider();
        let receipt = ReceiptBuilder::new()
            .transaction_hash(b256!(
                "0x0000000000000000000000000000000000000000000000000000000000000002"
            ))
            .block_number(1)
            .build();
        asserter.push_success(&receipt);

        let monitor_manager = create_test_monitor_manager(vec![], vec![]);
        let source = EvmRpcSource::new(provider, monitor_manager);

        let tx_hashes = &[receipt.transaction_hash];
        let receipts = source.fetch_receipts(tx_hashes).await.unwrap();

        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts.get(&receipt.transaction_hash), Some(&receipt));
    }

    #[tokio::test]
    async fn test_fetch_receipts_partial_success() {
        let (provider, asserter) = mock_provider();
        let receipt =
            ReceiptBuilder::new().transaction_hash(B256::default()).block_number(1).build();

        asserter.push_success(&Option::<TransactionReceipt>::None);
        asserter.push_success(&receipt);

        let monitor_manager = create_test_monitor_manager(vec![], vec![]);
        let source = EvmRpcSource::new(provider, monitor_manager);

        let tx_hashes = &[B256::default(), receipt.transaction_hash];
        let receipts = source.fetch_receipts(tx_hashes).await.unwrap();

        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts.get(&receipt.transaction_hash), Some(&receipt));
    }

    #[tokio::test]
    async fn test_fetch_receipts_error_handling() {
        let (provider, asserter) = mock_provider();
        asserter.push_failure_msg("RPC error");

        let monitor_manager = create_test_monitor_manager(vec![], vec![]);
        let source = EvmRpcSource::new(provider, monitor_manager);

        let tx_hashes = &[B256::default()];
        let result = source.fetch_receipts(tx_hashes).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_provider_success() {
        let url = Url::from_str("http://localhost:8545").unwrap();
        let retry_config = RpcRetryConfig::default();

        let result = create_provider(vec![url], retry_config);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_create_provider_error_handling() {
        let retry_config = RpcRetryConfig::default();
        let result = create_provider(vec![], retry_config);
        assert!(matches!(result, Err(ProviderError::CreationError(_))));
    }
}
