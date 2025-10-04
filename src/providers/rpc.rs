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
use async_trait::async_trait;
use tower::ServiceBuilder;

use super::traits::{DataSource, DataSourceError};
use crate::{config::RpcRetryConfig, monitor::MonitorManager};

/// A `DataSource` implementation that fetches data from an EVM RPC endpoint.
pub struct EvmRpcSource {
    /// The RPC provider used to fetch block data.
    provider: Arc<dyn Provider + Send + Sync>,

    /// The monitor manager to access Interest registry
    monitor_manager: Arc<MonitorManager>,
}

impl EvmRpcSource {
    /// Creates a new `EvmRpcSource`.
    #[tracing::instrument(skip(provider), level = "debug")]
    pub fn new(
        provider: Arc<dyn Provider + Send + Sync>,
        monitor_manager: Arc<MonitorManager>,
    ) -> Self {
        Self { provider, monitor_manager }
    }
}

#[async_trait]
impl DataSource for EvmRpcSource {
    #[tracing::instrument(skip(self), level = "debug")]
    async fn fetch_block_core_data(
        &self,
        block_number: u64,
    ) -> Result<(Block, Vec<Log>), DataSourceError> {
        tracing::debug!(block_number, "Fetching core block data.");
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
        let monitor_snapshot = self.monitor_manager.load();
        let interest_registry = &monitor_snapshot.interest_registry;

        // Early exit if there are absolutely no log-aware monitors of any kind.
        if interest_registry.log_interests.is_empty()
            && interest_registry.global_event_signatures.is_empty()
        {
            tracing::debug!(block_number = number, "No log-aware monitors. Skipping log fetch.");
            return Ok((block, Vec::new()));
        }

        // Debug interest registry contents for CI debugging
        tracing::info!(
            block_number = number,
            log_interests_count = interest_registry.log_interests.len(),
            global_event_signatures_count = interest_registry.global_event_signatures.len(),
            "Interest registry state"
        );
        for (addr, mode) in interest_registry.log_interests.iter() {
            match mode {
                Some(signatures) => tracing::info!(
                    address = %addr,
                    signature_count = signatures.len(),
                    "Address-specific interest (precise mode)"
                ),
                None => tracing::info!(
                    address = %addr,
                    "Address-specific interest (broad mode)"
                ),
            }
        }

        // Check 1: Do any globally monitored topics appear in the bloom?
        let might_have_global_logs = interest_registry
            .global_event_signatures
            .iter()
            .any(|topic| block_bloom.contains_input(BloomInput::Raw(topic.as_slice())));
        tracing::debug!(might_have_global_logs, "Checked for global log interests.");

        // Check 2: Do any address-specific interests appear in the bloom?
        let might_have_address_logs =
            interest_registry.log_interests.iter().any(|(addr, interest_mode)| {
                let address_found = block_bloom.contains_input(BloomInput::Raw(addr.as_slice()));
                if !address_found {
                    tracing::trace!(address = %addr, "Address not found in bloom filter.");
                    return false;
                }

                tracing::debug!(address = %addr, "Address found in bloom filter. Checking topics...");
                match interest_mode {
                    Some(specific_signatures) => {
                        let topics_found = specific_signatures.iter().any(|topic| {
                            let found = block_bloom.contains_input(BloomInput::Raw(topic.as_slice()));
                            tracing::trace!(topic = %topic, found, "Checked topic for address.");
                            found
                        });
                        if topics_found {
                            tracing::debug!(address = %addr, "Relevant topic found for address.");
                        }
                        topics_found
                    }
                    None => {
                        tracing::debug!(address = %addr, "No specific topics (broad mode), proceeding to fetch.");
                        true
                    }
                }
            });
        tracing::debug!(might_have_address_logs, "Checked for address-specific log interests.");

        let might_contain_relevant_logs = might_have_global_logs || might_have_address_logs;

        tracing::info!(
            block_number = number,
            might_have_global_logs,
            might_have_address_logs,
            might_contain_relevant_logs,
            "Bloom filter analysis"
        );

        // Conditionally call eth_getLogs based on the bloom filter check.
        let logs = if might_contain_relevant_logs {
            // The bloom filter indicates a potential match. We MUST fetch the logs to
            // verify.
            tracing::info!(block_number = number, "Bloom filter hit. Fetching logs.");
            self.fetch_logs_for_block(number).await?
        } else {
            // The bloom filter guarantees no relevant logs are in this block.
            // We can safely skip the expensive eth_getLogs call.
            tracing::info!(block_number = number, "Bloom filter miss. Skipping log fetch.");
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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use alloy::primitives::{Address, B256, Bloom, BloomInput, U256, address, b256};

    use super::*;
    use crate::{
        models::monitor::Monitor,
        test_helpers::{
            BlockBuilder, LogBuilder, ReceiptBuilder, create_test_monitor_manager, mock_provider,
        },
    };

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

        let monitor = Monitor {
            name: "Test Monitor".into(),
            address: Some(monitored_address.to_string()),
            filter_script: "log != ()".into(), // Should be log-aware monitor
            ..Default::default()
        };

        let monitor_manager = create_test_monitor_manager(vec![monitor]);
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

        let monitor_manager = create_test_monitor_manager(vec![]);
        let source = EvmRpcSource::new(provider, monitor_manager);

        let result = source.fetch_block_core_data(1).await;

        assert!(matches!(result, Err(DataSourceError::BlockNotFound(1))));
    }

    #[tokio::test]
    async fn test_fetch_block_core_data_error_handling() {
        let (provider, asserter) = mock_provider();
        asserter.push_failure_msg("RPC error");
        asserter.push_success(&Vec::<Log>::new());

        let monitor_manager = create_test_monitor_manager(vec![]);
        let source = EvmRpcSource::new(provider, monitor_manager);

        let result = source.fetch_block_core_data(1).await;

        assert!(matches!(result, Err(DataSourceError::Provider(_))));
    }

    #[tokio::test]
    async fn test_get_current_block_number() {
        let (provider, asserter) = mock_provider();
        asserter.push_success(&U256::from(1));

        let monitor_manager = create_test_monitor_manager(vec![]);
        let source = EvmRpcSource::new(provider, monitor_manager);

        let block_number = source.get_current_block_number().await.unwrap();

        assert_eq!(block_number, 1);
    }

    #[tokio::test]
    async fn test_get_current_block_number_error_handling() {
        let (provider, asserter) = mock_provider();
        asserter.push_failure_msg("RPC error");

        let monitor_manager = create_test_monitor_manager(vec![]);
        let source = EvmRpcSource::new(provider, monitor_manager);

        let result = source.get_current_block_number().await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_fetch_receipts_error_handling() {
        let (provider, asserter) = mock_provider();
        asserter.push_failure_msg("RPC error");

        let monitor_manager = create_test_monitor_manager(vec![]);
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

    #[tokio::test]
    async fn test_fetch_block_and_logs_bloom_hit_address() {
        let (provider, asserter) = mock_provider();
        let block_number = 123;
        let monitored_address = Address::default();

        let mut bloom = Bloom::default();
        bloom.accrue(BloomInput::Raw(monitored_address.as_slice()));

        let block = BlockBuilder::new().number(block_number).bloom(bloom).build();
        let logs: Vec<Log> = vec![Log::default(), Log::default()];

        asserter.push_success(&block);
        asserter.push_success(&logs);

        let monitor = Monitor {
            name: "Test Monitor".into(),
            network: "testnet".into(),
            address: Some(monitored_address.to_string()),
            filter_script: "log != ()".to_string(), // should be log-aware
            ..Default::default()
        };

        let monitor_manager = create_test_monitor_manager(vec![monitor]);
        let data_source = EvmRpcSource::new(provider, monitor_manager);
        let (fetched_block, fetched_logs) =
            data_source.fetch_block_and_logs(block_number).await.unwrap();

        assert_eq!(fetched_block.header.number, block_number);
        assert_eq!(fetched_logs.len(), 2);
    }

    #[tokio::test]
    async fn test_fetch_block_and_logs_bloom_hit_topic() {
        let (provider, asserter) = mock_provider();
        let block_number = 123;
        let transfer_topic =
            b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"); // keccak256("Transfer(address,address,uint256)")

        let mut bloom = Bloom::default();
        bloom.accrue(BloomInput::Raw(transfer_topic.as_slice()));

        let block = BlockBuilder::new().number(block_number).bloom(bloom).build();
        let log = LogBuilder::new().topics(vec![transfer_topic]).build();

        asserter.push_success(&block);
        asserter.push_success(&vec![log]);

        let monitor = Monitor {
            name: "Test Monitor".into(),
            network: "testnet".into(),
            address: Some("all".to_string()), // 'all' indicates global event signature monitoring
            abi: Some("erc20".to_string()),   // ABI with Transfer event
            filter_script: "log.name == \"Transfer\"".to_string(),
            ..Default::default()
        };

        let monitor_manager = create_test_monitor_manager(vec![monitor]);
        let data_source = EvmRpcSource::new(provider, monitor_manager);
        let (fetched_block, fetched_logs) =
            data_source.fetch_block_and_logs(block_number).await.unwrap();

        assert_eq!(fetched_block.header.number, block_number);
        assert_eq!(fetched_logs.len(), 1);
    }

    #[tokio::test]
    async fn test_fetch_block_and_logs_bloom_miss() {
        let (provider, asserter) = mock_provider();
        let block_number = 123;
        let monitored_address = address!("1111111111111111111111111111111111111111");

        let block = BlockBuilder::new().number(block_number).build(); // Default bloom is empty

        asserter.push_success(&block);

        let monitor = Monitor {
            name: "Test Monitor".into(),
            network: "testnet".into(),
            address: Some(monitored_address.to_string()),
            filter_script: "true".to_string(),
            ..Default::default()
        };

        let monitor_manager = create_test_monitor_manager(vec![monitor]);
        let data_source = EvmRpcSource::new(provider, monitor_manager);
        let (fetched_block, fetched_logs) =
            data_source.fetch_block_and_logs(block_number).await.unwrap();

        assert_eq!(fetched_block.header.number, block_number);
        assert!(fetched_logs.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_receipts_success() {
        let (provider, asserter) = mock_provider();
        let tx_hash1 = B256::from_slice(&[1; 32]);
        let tx_hash2 = B256::from_slice(&[2; 32]);

        let receipt1 = ReceiptBuilder::new().transaction_hash(tx_hash1).build();
        let receipt2 = ReceiptBuilder::new().transaction_hash(tx_hash2).build();

        // Push responses in the order they are expected to be called.
        asserter.push_success(&receipt1);
        asserter.push_success(&receipt2);

        let monitor_manager = create_test_monitor_manager(vec![]);
        let data_source = EvmRpcSource::new(provider, monitor_manager);
        let receipts = data_source.fetch_receipts(&[tx_hash1, tx_hash2]).await.unwrap();

        assert_eq!(receipts.len(), 2);
        assert_eq!(receipts.get(&tx_hash1).unwrap().transaction_hash, tx_hash1);
        assert_eq!(receipts.get(&tx_hash2).unwrap().transaction_hash, tx_hash2);
    }

    #[tokio::test]
    async fn test_fetch_receipts_empty() {
        let (provider, _) = mock_provider(); // Asserter is not needed as no calls are made.     
        let monitor_manager = create_test_monitor_manager(vec![]);
        let data_source = EvmRpcSource::new(provider, monitor_manager);
        let receipts = data_source.fetch_receipts(&[]).await.unwrap();
        assert!(receipts.is_empty());
    }

    #[tokio::test]
    async fn test_get_current_block_number_success() {
        let (provider, asserter) = mock_provider();
        let current_block = 999;

        asserter.push_success(&U256::from(current_block));

        let monitor_manager = create_test_monitor_manager(vec![]);
        let data_source = EvmRpcSource::new(provider, monitor_manager);
        let result = data_source.get_current_block_number().await.unwrap();

        assert_eq!(result, current_block);
    }

    #[tokio::test]
    async fn test_fetch_block_not_found() {
        let (provider, asserter) = mock_provider();
        let block_number = 404;

        // Mock a `null` response for the block, which deserializes to `None`.
        asserter.push_success(&Option::<Block>::None);
        // The logs request will still be made in the sequential test version.
        asserter.push_success(&Vec::<Log>::new());

        let monitor_manager = create_test_monitor_manager(vec![]);
        let data_source = EvmRpcSource::new(provider, monitor_manager);
        let result = data_source.fetch_block_and_logs(block_number).await;

        assert!(matches!(result, Err(DataSourceError::BlockNotFound(404))));
    }

    #[tokio::test]
    async fn test_fetch_receipts_partial_success() {
        let (provider, asserter) = mock_provider();
        let tx_hash1 = B256::from_slice(&[1; 32]);
        let tx_hash2 = B256::from_slice(&[2; 32]); // This one will not be found.

        let receipt1 = ReceiptBuilder::new().transaction_hash(tx_hash1).build();

        // Push a success for the first receipt.
        asserter.push_success(&receipt1);
        // Push a `null` response for the second, which deserializes to `None`.
        asserter.push_success(&Option::<TransactionReceipt>::None);

        let monitor_manager = create_test_monitor_manager(vec![]);
        let data_source = EvmRpcSource::new(provider, monitor_manager);
        let receipts = data_source.fetch_receipts(&[tx_hash1, tx_hash2]).await.unwrap();

        assert_eq!(receipts.len(), 1);
        assert!(receipts.contains_key(&tx_hash1));
        assert!(!receipts.contains_key(&tx_hash2));
    }

    #[tokio::test]
    async fn test_provider_error_propagation() {
        let (provider, asserter) = mock_provider();

        // Push a custom error response.
        asserter.push_failure_msg("test provider error");

        let monitor_manager = create_test_monitor_manager(vec![]);
        let data_source = EvmRpcSource::new(provider, monitor_manager);
        let result = data_source.get_current_block_number().await;

        assert!(matches!(result, Err(DataSourceError::Provider(_))));
        assert!(result.unwrap_err().to_string().contains("test provider error"));
    }

    #[tokio::test]
    async fn test_fetch_block_and_logs_no_log_interest() {
        let (provider, asserter) = mock_provider();
        let block_number = 123;

        // Block can have a bloom filter, it shouldn't matter.
        let mut bloom = Bloom::default();
        bloom.accrue(BloomInput::Raw(&[1; 32]));
        let block = BlockBuilder::new().number(block_number).bloom(bloom).build();

        // Only push the block response. The logs response should never be requested.
        asserter.push_success(&block);

        // Create a monitor that is NOT log-aware (e.g., it only checks tx.value)
        let monitor = Monitor {
            name: "TX Only Monitor".into(),
            network: "testnet".into(),
            filter_script: "tx.value > 100".to_string(),
            ..Default::default()
        };

        let monitor_manager = create_test_monitor_manager(vec![monitor]);
        let data_source = EvmRpcSource::new(provider, monitor_manager);
        let (fetched_block, fetched_logs) =
            data_source.fetch_block_and_logs(block_number).await.unwrap();

        assert_eq!(fetched_block.header.number, block_number);
        // The key assertion: logs are empty because they were never fetched.
        assert!(fetched_logs.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_block_and_logs_log_fetch_fails() {
        let (provider, asserter) = mock_provider();
        let block_number = 123;
        let monitored_address = Address::default();

        let mut bloom = Bloom::default();
        bloom.accrue(BloomInput::Raw(monitored_address.as_slice()));
        let block = BlockBuilder::new().number(block_number).bloom(bloom).build();

        // Mock a successful block response.
        asserter.push_success(&block);
        // Mock a failure for the logs response.
        asserter.push_failure_msg("failed to get logs");

        let monitor = Monitor {
            name: "Test Monitor".into(),
            network: "testnet".into(),
            address: Some(monitored_address.to_string()),
            filter_script: "log != ()".to_string(),
            ..Default::default()
        };

        let monitor_manager = create_test_monitor_manager(vec![monitor]);
        let data_source = EvmRpcSource::new(provider, monitor_manager);
        let result = data_source.fetch_block_and_logs(block_number).await;

        assert!(matches!(result, Err(DataSourceError::Provider(_))));

        assert!(result.unwrap_err().to_string().contains("failed to get logs",));
    }

    #[tokio::test]
    async fn test_fetch_receipts_provider_error() {
        let (provider, asserter) = mock_provider();
        let tx_hash1 = B256::from_slice(&[1; 32]);
        let tx_hash2 = B256::from_slice(&[2; 32]);

        let receipt1 = ReceiptBuilder::new().transaction_hash(tx_hash1).build();

        // Push a success for the first receipt.
        asserter.push_success(&receipt1);
        // Push a failure for the second receipt.
        asserter.push_failure_msg("receipt unavailable");

        let monitor_manager = create_test_monitor_manager(vec![]);
        let data_source = EvmRpcSource::new(provider, monitor_manager);
        let result = data_source.fetch_receipts(&[tx_hash1, tx_hash2]).await;

        assert!(matches!(result, Err(DataSourceError::Provider(_))));
        assert!(result.unwrap_err().to_string().contains("receipt unavailable"));
    }
}
