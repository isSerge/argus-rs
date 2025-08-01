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

    /// Fetches only the transaction receipts for a given list of transaction hashes.
    ///
    /// This method leverages the provider's `CallBatchLayer` (if configured)
    /// to automatically batch these requests.
    #[tracing::instrument(skip(self), level = "debug")]
    pub async fn fetch_receipts(
        &self,
        tx_hashes: &[TxHash],
    ) -> Result<HashMap<TxHash, TransactionReceipt>, BlockFetcherError> {
        if tx_hashes.is_empty() {
            return Ok(HashMap::new());
        }

        // TODO: verify requests are batched correctly by the provider.
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
