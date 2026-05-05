//! This module provides functionality to create a provider for EVM RPC requests
//! with retry logic and backoff strategies.

use std::{collections::HashMap, num::NonZeroUsize, sync::Arc};

use alloy::{
    primitives::{BloomInput, TxHash},
    providers::{Provider, ProviderBuilder, layers::CallBatchLayer},
    rpc::{
        client::RpcClient,
        types::{Block, Filter, Log, TransactionReceipt},
    },
    transports::{
        http::{Http, reqwest::Url},
        layers::{FallbackLayer, RetryBackoffLayer},
    },
};
use arc_swap::ArcSwap;
use argus_core::{
    config::RpcRetryConfig,
    monitor::InterestRegistry,
    providers::traits::{DataSource, DataSourceError},
};
use async_trait::async_trait;
use tower::ServiceBuilder;

/// A `DataSource` implementation that fetches data from an EVM RPC endpoint.
pub struct EvmRpcSource {
    /// The RPC provider used to fetch block data.
    provider: Arc<dyn Provider + Send + Sync>,

    /// Shared interest registry for bloom-filter pre-screening.
    interest_registry: Arc<ArcSwap<InterestRegistry>>,
}

impl EvmRpcSource {
    /// Creates a new `EvmRpcSource`.
    #[tracing::instrument(skip(provider), level = "debug")]
    pub fn new(
        provider: Arc<dyn Provider + Send + Sync>,
        interest_registry: Arc<ArcSwap<InterestRegistry>>,
    ) -> Self {
        Self { provider, interest_registry }
    }
}

#[async_trait]
impl DataSource for EvmRpcSource {
    #[tracing::instrument(skip(self), level = "debug")]
    async fn fetch_block_core_data(
        &self,
        block_number: u64,
    ) -> Result<(Block, Vec<Log>), DataSourceError> {
        match self.fetch_block_and_logs(block_number).await {
            Ok(data) => {
                tracing::debug!(block_number, "Successfully fetched core block data.");
                Ok(data)
            }
            Err(DataSourceError::BlockNotFound(num)) => {
                tracing::warn!(block_number = num, "Block not found.");
                Err(DataSourceError::BlockNotFound(num))
            }
            Err(e) => {
                tracing::error!(error = %e, block_number, "Failed to fetch core block data.");
                Err(DataSourceError::Provider(Box::new(e)))
            }
        }
    }

    /// Fetches only the transaction receipts for a given list of transaction
    /// hashes.
    ///
    /// This method leverages the provider's `CallBatchLayer`
    /// to automatically batch these requests.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn fetch_receipts(
        &self,
        tx_hashes: &[TxHash],
    ) -> Result<HashMap<TxHash, TransactionReceipt>, DataSourceError> {
        if tx_hashes.is_empty() {
            return Ok(HashMap::new());
        }

        let futures = tx_hashes.iter().map(|&tx_hash| async move {
            let receipt = self
                .provider
                .get_transaction_receipt(tx_hash)
                .await
                .map_err(|e| DataSourceError::Provider(Box::new(e)))?;
            Ok::<_, DataSourceError>((tx_hash, receipt))
        });

        let results = futures::future::try_join_all(futures).await?;

        let receipts = results
            .into_iter()
            .filter_map(|(tx_hash, receipt)| receipt.map(|r| (tx_hash, r)))
            .collect();

        Ok(receipts)
    }

    /// Fetches the current block number from the data source.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn get_current_block_number(&self) -> Result<u64, DataSourceError> {
        self.provider.get_block_number().await.map_err(|e| DataSourceError::Provider(Box::new(e)))
    }
}

impl EvmRpcSource {
    /// Fetches all logs for a given block number.
    async fn fetch_logs_for_block(&self, number: u64) -> Result<Vec<Log>, DataSourceError> {
        let filter = Filter::new().from_block(number).to_block(number);
        self.provider.get_logs(&filter).await.map_err(|e| DataSourceError::Provider(Box::new(e)))
    }

    /// Fetches the core data for a block (the block itself and all its logs).
    ///
    /// This method does NOT fetch transaction receipts, which must be fetched
    /// separately if required.
    #[tracing::instrument(skip(self), level = "debug")]
    pub async fn fetch_block_and_logs(
        &self,
        number: u64,
    ) -> Result<(Block, Vec<Log>), DataSourceError> {
        // Fetch block first
        let block = self
            .provider
            .get_block_by_number(number.into())
            .full()
            .await
            .map_err(|e| DataSourceError::Provider(Box::new(e)))?
            .ok_or(DataSourceError::BlockNotFound(number))?;

        // Check if there is any log interest in this block
        // If not, skip fetching logs to save RPC calls
        let block_bloom = &block.header.logs_bloom;
        let interest_registry = self.interest_registry.load();

        // Early exit if there are absolutely no log-aware monitors of any kind.
        if interest_registry.log_interests.is_empty()
            && interest_registry.global_event_signatures.is_empty()
        {
            tracing::debug!(block_number = number, "No log-aware monitors. Skipping log fetch.");
            return Ok((block, Vec::new()));
        }

        // Check 1: Do any globally monitored topics appear in the bloom?
        let might_have_global_logs = interest_registry
            .global_event_signatures
            .iter()
            .any(|topic| block_bloom.contains_input(BloomInput::Raw(topic.as_slice())));

        // Check 2: Do any address-specific interests appear in the bloom?
        let might_have_address_logs =
            interest_registry.log_interests.iter().any(|(addr, interest_mode)| {
                // All address-specific checks must first match the address in the bloom.
                if !block_bloom.contains_input(BloomInput::Raw(addr.as_slice())) {
                    return false;
                }

                match interest_mode {
                    // Precise Mode: Address is present, now check if any of its specific topics are
                    // also present.
                    Some(specific_signatures) => specific_signatures
                        .iter()
                        .any(|topic| block_bloom.contains_input(BloomInput::Raw(topic.as_slice()))),
                    // Broad Mode: Address is present, and since we can't be more specific, we must
                    // fetch.
                    None => true,
                }
            });

        let might_contain_relevant_logs = might_have_global_logs || might_have_address_logs;

        // Conditionally call eth_getLogs based on the bloom filter check.
        let logs = if might_contain_relevant_logs {
            // The bloom filter indicates a potential match. We MUST fetch the logs to
            // verify.
            tracing::debug!(block_number = number, "Bloom filter hit. Fetching logs.");
            self.fetch_logs_for_block(number).await?
        } else {
            // The bloom filter guarantees no relevant logs are in this block.
            // We can safely skip the expensive eth_getLogs call.
            tracing::debug!(block_number = number, "Bloom filter miss. Skipping log fetch.");
            Vec::new()
        };

        Ok((block, logs))
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
