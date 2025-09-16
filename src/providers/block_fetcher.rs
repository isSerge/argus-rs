//! This module contains the `BlockFetcher` component, responsible for
//! efficiently retrieving all necessary data for a given block from an EVM RPC
//! endpoint.

use std::{collections::HashMap, sync::Arc};

use alloy::{
    primitives::{BloomInput, TxHash},
    providers::Provider,
    rpc::types::{Block, Filter, Log, TransactionReceipt},
};
use thiserror::Error;

use crate::monitor::MonitorManager;

/// Custom error type for the `BlockFetcher`.
#[derive(Error, Debug)]
pub enum BlockFetcherError {
    /// Error when interacting with the RPC provider.
    #[error("Provider error: {0}")]
    Provider(#[from] Box<dyn std::error::Error + Send + Sync>),
    /// Indicates that the requested block was not found.
    #[error("Block not found: {0}")]
    BlockNotFound(u64),
}

/// A component responsible for fetching all data related to a single block.
pub struct BlockFetcher<P> {
    /// The RPC provider used to fetch block data.
    provider: P,

    /// The monitor manager to access Interest registry
    monitor_manager: Arc<MonitorManager>,
}

impl<P> BlockFetcher<P>
where
    P: Provider + Send + Sync,
{
    /// Creates a new `BlockFetcher`.
    pub fn new(provider: P, monitor_manager: Arc<MonitorManager>) -> Self {
        Self { provider, monitor_manager }
    }

    /// Fetches the core data for a block (the block itself and all its logs).
    ///
    /// This method does NOT fetch transaction receipts, which must be fetched
    /// separately if required.
    #[tracing::instrument(skip(self), level = "debug")]
    pub async fn fetch_block_and_logs(
        &self,
        number: u64,
    ) -> Result<(Block, Vec<Log>), BlockFetcherError> {
        // Fetch block first
        let block = self
            .provider
            .get_block_by_number(number.into())
            .full()
            .await
            .map_err(|e| BlockFetcherError::Provider(Box::new(e)))?
            .ok_or(BlockFetcherError::BlockNotFound(number))?;

        // Check if there is any log interest in this block
        // If not, skip fetching logs to save RPC calls
        let block_bloom = &block.header.logs_bloom;
        let monitor_snapshot = self.monitor_manager.load();
        let has_log_interest = !monitor_snapshot.interest_registry.log_addresses.is_empty()
            || !monitor_snapshot.interest_registry.global_event_signatures.is_empty();

        if !has_log_interest {
            tracing::debug!(block_number = number, "No log-aware monitors. Skipping log fetch.");
            return Ok((block, Vec::new()));
        }

        // If there is log interest, check the bloom filter
        // Check if any monitored address is possibly in the block's logs.
        let has_any_monitored_address = monitor_snapshot
            .interest_registry
            .log_addresses
            .iter()
            .any(|addr| block_bloom.contains_input(BloomInput::Raw(addr.as_slice())));

        // Check if any globally monitored topic is possibly in the block's logs.
        let has_any_monitored_topic = monitor_snapshot
            .interest_registry
            .global_event_signatures
            .iter()
            .any(|topic| block_bloom.contains_input(BloomInput::Raw(topic.as_slice())));

        let might_contain_relevant_logs = has_any_monitored_address || has_any_monitored_topic;

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

    /// Fetches only the transaction receipts for a given list of transaction
    /// hashes.
    ///
    /// This method leverages the provider's `CallBatchLayer`
    /// to automatically batch these requests.
    #[tracing::instrument(skip(self), level = "debug")]
    pub async fn fetch_receipts(
        &self,
        tx_hashes: &[TxHash],
    ) -> Result<HashMap<TxHash, TransactionReceipt>, BlockFetcherError> {
        if tx_hashes.is_empty() {
            return Ok(HashMap::new());
        }

        let futures = tx_hashes.iter().map(|&tx_hash| async move {
            let receipt = self
                .provider
                .get_transaction_receipt(tx_hash)
                .await
                .map_err(|e| BlockFetcherError::Provider(Box::new(e)))?;
            Ok::<_, BlockFetcherError>((tx_hash, receipt))
        });

        let results = futures::future::try_join_all(futures).await?;

        let receipts = results
            .into_iter()
            .filter_map(|(tx_hash, receipt)| receipt.map(|r| (tx_hash, r)))
            .collect();

        Ok(receipts)
    }

    /// Fetches all logs for a given block number.
    async fn fetch_logs_for_block(&self, number: u64) -> Result<Vec<Log>, BlockFetcherError> {
        let filter = Filter::new().from_block(number).to_block(number);
        self.provider.get_logs(&filter).await.map_err(|e| BlockFetcherError::Provider(Box::new(e)))
    }

    /// Fetches the current block number from the data source.
    #[tracing::instrument(skip(self), level = "debug")]
    pub async fn get_current_block_number(&self) -> Result<u64, BlockFetcherError> {
        self.provider.get_block_number().await.map_err(|e| BlockFetcherError::Provider(Box::new(e)))
    }
}

#[cfg(test)]
mod tests {
    use alloy::{
        network::Ethereum,
        primitives::{Address, B256, Bloom, U256, address, b256},
        providers::{Provider, ProviderBuilder},
        rpc::types::{Block, Log},
        transports::mock::Asserter,
    };

    use super::*;
    use crate::{
        models::monitor::Monitor,
        test_helpers::{BlockBuilder, LogBuilder, ReceiptBuilder, create_test_monitor_manager},
    };

    // Helper to create a provider and asserter from the user's example.
    fn mock_provider() -> (impl Provider<Ethereum>, Asserter) {
        let asserter = Asserter::new();
        let provider = ProviderBuilder::new().connect_mocked_client(asserter.clone());
        (provider, asserter)
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
            filter_script: "log != ()".to_string(), // should be log-aware monitor
            ..Default::default()
        };

        let monitor_manager = create_test_monitor_manager(vec![monitor]);
        let fetcher = BlockFetcher::new(provider, monitor_manager);
        let (fetched_block, fetched_logs) =
            fetcher.fetch_block_and_logs(block_number).await.unwrap();

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
            abi: Some("simple".to_string()),  // ABI with Transfer event
            filter_script: "log.name == \"Transfer\"".to_string(),
            ..Default::default()
        };

        let monitor_manager = create_test_monitor_manager(vec![monitor]);
        let fetcher = BlockFetcher::new(provider, monitor_manager);
        let (fetched_block, fetched_logs) =
            fetcher.fetch_block_and_logs(block_number).await.unwrap();

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
        let fetcher = BlockFetcher::new(provider, monitor_manager);
        let (fetched_block, fetched_logs) =
            fetcher.fetch_block_and_logs(block_number).await.unwrap();

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
        let fetcher = BlockFetcher::new(provider, monitor_manager);
        let receipts = fetcher.fetch_receipts(&[tx_hash1, tx_hash2]).await.unwrap();

        assert_eq!(receipts.len(), 2);
        assert_eq!(receipts.get(&tx_hash1).unwrap().transaction_hash, tx_hash1);
        assert_eq!(receipts.get(&tx_hash2).unwrap().transaction_hash, tx_hash2);
    }

    #[tokio::test]
    async fn test_fetch_receipts_empty() {
        let (provider, _) = mock_provider(); // Asserter is not needed as no calls are made.
        let monitor_manager = create_test_monitor_manager(vec![]);
        let fetcher = BlockFetcher::new(provider, monitor_manager);
        let receipts = fetcher.fetch_receipts(&[]).await.unwrap();
        assert!(receipts.is_empty());
    }

    #[tokio::test]
    async fn test_get_current_block_number_success() {
        let (provider, asserter) = mock_provider();
        let current_block = 999;

        asserter.push_success(&U256::from(current_block));

        let monitor_manager = create_test_monitor_manager(vec![]);
        let fetcher = BlockFetcher::new(provider, monitor_manager);
        let result = fetcher.get_current_block_number().await.unwrap();

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
        let fetcher = BlockFetcher::new(provider, monitor_manager);
        let result = fetcher.fetch_block_and_logs(block_number).await;

        assert!(matches!(result, Err(BlockFetcherError::BlockNotFound(404))));
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
        let fetcher = BlockFetcher::new(provider, monitor_manager);
        let receipts = fetcher.fetch_receipts(&[tx_hash1, tx_hash2]).await.unwrap();

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
        let fetcher = BlockFetcher::new(provider, monitor_manager);
        let result = fetcher.get_current_block_number().await;

        assert!(matches!(result, Err(BlockFetcherError::Provider(_))));
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
        let fetcher = BlockFetcher::new(provider, monitor_manager);
        let (fetched_block, fetched_logs) =
            fetcher.fetch_block_and_logs(block_number).await.unwrap();

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
        let fetcher = BlockFetcher::new(provider, monitor_manager);
        let result = fetcher.fetch_block_and_logs(block_number).await;

        assert!(matches!(result, Err(BlockFetcherError::Provider(_))));
        assert!(result.unwrap_err().to_string().contains("failed to get logs"));
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
        let fetcher = BlockFetcher::new(provider, monitor_manager);
        let result = fetcher.fetch_receipts(&[tx_hash1, tx_hash2]).await;

        assert!(matches!(result, Err(BlockFetcherError::Provider(_))));
        assert!(result.unwrap_err().to_string().contains("receipt unavailable"));
    }
}
