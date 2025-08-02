//! This module contains the `BlockFetcher` component, responsible for
//! efficiently retrieving all necessary data for a given block from an EVM RPC endpoint.

use alloy::{
    primitives::TxHash,
    providers::Provider,
    rpc::types::{Block, Filter, Log, TransactionReceipt},
};
use std::collections::HashMap;
use thiserror::Error;

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
    provider: P,
}

impl<P> BlockFetcher<P>
where
    P: Provider + Send + Sync,
{
    /// Creates a new `BlockFetcher`.
    pub fn new(provider: P) -> Self {
        Self { provider }
    }

    /// Fetches the core data for a block (the block itself and all its logs).
    ///
    /// This method does NOT fetch transaction receipts, which must be fetched
    /// separately if required.
    #[tracing::instrument(skip(self), level = "debug")]
    #[cfg(not(test))]
    pub async fn fetch_block_and_logs(
        &self,
        number: u64,
    ) -> Result<(Block, Vec<Log>), BlockFetcherError> {
        // Fetch the full block and all logs concurrently.
        let (block_result, logs_result) = tokio::join!(
            self.provider.get_block_by_number(number.into()).full(),
            self.fetch_logs_for_block(number)
        );

        let block = block_result
            .map_err(|e| BlockFetcherError::Provider(Box::new(e)))?
            .ok_or(BlockFetcherError::BlockNotFound(number))?;

        let logs = logs_result?;

        Ok((block, logs))
    }

    /// Test-only version of `fetch_block_and_logs` that runs sequentially.
    #[tracing::instrument(skip(self), level = "debug")]
    #[cfg(test)]
    pub async fn fetch_block_and_logs(
        &self,
        number: u64,
    ) -> Result<(Block, Vec<Log>), BlockFetcherError> {
        // Fetch the full block first, then the logs.
        let block = self
            .provider
            .get_block_by_number(number.into())
            .full()
            .await
            .map_err(|e| BlockFetcherError::Provider(Box::new(e)))?
            .ok_or(BlockFetcherError::BlockNotFound(number))?;

        let logs = self.fetch_logs_for_block(number).await?;

        Ok((block, logs))
    }

    /// Fetches only the transaction receipts for a given list of transaction hashes.
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
        self.provider
            .get_logs(&filter)
            .await
            .map_err(|e| BlockFetcherError::Provider(Box::new(e)))
    }

    /// Fetches the current block number from the data source.
    #[tracing::instrument(skip(self), level = "debug")]
    pub async fn get_current_block_number(&self) -> Result<u64, BlockFetcherError> {
        self.provider
            .get_block_number()
            .await
            .map_err(|e| BlockFetcherError::Provider(Box::new(e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::ReceiptBuilder;
    use alloy::{
        network::Ethereum,
        primitives::{B256, U256},
        providers::{Provider, ProviderBuilder},
        rpc::types::{Block, BlockTransactions, Header, Log},
        transports::mock::Asserter,
    };

    // Helper to create a provider and asserter from the user's example.
    fn mock_provider() -> (impl Provider<Ethereum>, Asserter) {
        let asserter = Asserter::new();
        let provider = ProviderBuilder::new().connect_mocked_client(asserter.clone());
        (provider, asserter)
    }

    fn create_test_block(block_number: u64) -> Block {
        let mut header: Header = Header::default();
        header.inner.number = block_number.into();

        Block {
            header,
            transactions: BlockTransactions::Hashes(vec![]),
            uncles: vec![],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_fetch_block_and_logs_success() {
        let (provider, asserter) = mock_provider();
        let block_number = 123;

        let block = create_test_block(block_number);
        let logs: Vec<Log> = vec![Log::default(), Log::default()];

        // Test is using the sequential implementation of `fetch_block_and_logs`.
        asserter.push_success(&block);
        asserter.push_success(&logs);

        let fetcher = BlockFetcher::new(provider);
        let (fetched_block, fetched_logs) =
            fetcher.fetch_block_and_logs(block_number).await.unwrap();

        assert_eq!(fetched_block.header.number, block_number);
        assert_eq!(fetched_logs.len(), 2);
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

        let fetcher = BlockFetcher::new(provider);
        let receipts = fetcher.fetch_receipts(&[tx_hash1, tx_hash2]).await.unwrap();

        assert_eq!(receipts.len(), 2);
        assert_eq!(receipts.get(&tx_hash1).unwrap().transaction_hash, tx_hash1);
        assert_eq!(receipts.get(&tx_hash2).unwrap().transaction_hash, tx_hash2);
    }

    #[tokio::test]
    async fn test_fetch_receipts_empty() {
        let (provider, _) = mock_provider(); // Asserter is not needed as no calls are made.
        let fetcher = BlockFetcher::new(provider);
        let receipts = fetcher.fetch_receipts(&[]).await.unwrap();
        assert!(receipts.is_empty());
    }

    #[tokio::test]
    async fn test_get_current_block_number_success() {
        let (provider, asserter) = mock_provider();
        let current_block = 999;

        asserter.push_success(&U256::from(current_block));

        let fetcher = BlockFetcher::new(provider);
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

        let fetcher = BlockFetcher::new(provider);
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

        let fetcher = BlockFetcher::new(provider);
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

        let fetcher = BlockFetcher::new(provider);
        let result = fetcher.get_current_block_number().await;

        assert!(matches!(result, Err(BlockFetcherError::Provider(_))));
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("test provider error")
        );
    }
}
