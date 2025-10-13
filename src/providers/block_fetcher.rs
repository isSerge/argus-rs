//! This module provides reusable functions for fetching block data from a
//! DataSource. This is used by the BlockIngestor and Dry-run command to
//! retrieve block data concurrently.

use std::collections::HashMap;

use futures::stream::{self, StreamExt};

use crate::{
    models::BlockData,
    providers::traits::{DataSource, DataSourceError},
};

/// Fetches all necessary data for a single block.
pub async fn fetch_single_block_data<D: DataSource + ?Sized>(
    data_source: &D,
    needs_receipts: &bool,
    block_num: u64,
) -> Result<BlockData, DataSourceError> {
    let (block, logs) = data_source.fetch_block_core_data(block_num).await?;
    let receipts = if *needs_receipts {
        let tx_hashes: Vec<_> = block.transactions.hashes().collect();
        if tx_hashes.is_empty() {
            HashMap::new()
        } else {
            data_source.fetch_receipts(&tx_hashes).await?
        }
    } else {
        HashMap::new()
    };

    Ok(BlockData::from_raw_data(block, receipts, logs))
}

/// Fetches a range of blocks concurrently.
///
/// Returns an error if any block fails to fetch. This ensures consistent
/// behavior across all components and prevents gaps in block processing.
pub async fn fetch_blocks_concurrent<D: DataSource + ?Sized>(
    data_source: &D,
    needs_receipts: &bool,
    from_block: u64,
    to_block: u64,
    concurrency: usize,
) -> Result<Vec<BlockData>, DataSourceError> {
    let block_stream = stream::iter(from_block..=to_block).map(|block_num| async move {
        fetch_single_block_data(data_source, needs_receipts, block_num).await
    });

    let mut buffered_stream = block_stream.buffered(concurrency);
    let mut successful_blocks = Vec::new();

    while let Some(result) = buffered_stream.next().await {
        match result {
            Ok(block_data) => {
                successful_blocks.push(block_data);
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    from_block = from_block,
                    to_block = to_block,
                    "Failed to fetch block in range"
                );
                return Err(e);
            }
        }
    }

    // Sort by block number to ensure correct order (concurrent fetching may return
    // out of order)
    successful_blocks.sort_by_key(|block| block.block.header.number);

    Ok(successful_blocks)
}
