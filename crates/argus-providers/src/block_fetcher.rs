//! This module provides reusable functions for fetching block data from a
//! DataSource. This is used by the BlockIngestor and Dry-run command to
//! retrieve block data concurrently.

use std::collections::HashMap;

use alloy::rpc::types::Block;
use argus_core::{
    models::BlockData,
    providers::traits::{DataSource, DataSourceError},
};
use futures::{
    join,
    stream::{self, StreamExt},
};

/// Fetches all necessary data for a single block (used by legacy paths that
/// need per-block logs, e.g. the live ingestor when no range fetch is desired).
pub async fn fetch_single_block_data<D: DataSource + ?Sized>(
    data_source: &D,
    needs_receipts: bool,
    block_num: u64,
) -> Result<BlockData, DataSourceError> {
    let (block, logs) = data_source.fetch_block_core_data(block_num).await?;
    let receipts = if needs_receipts {
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

/// Fetches all blocks (without logs) for a range concurrently.
async fn fetch_blocks_only<D: DataSource + ?Sized>(
    data_source: &D,
    from_block: u64,
    to_block: u64,
    concurrency: usize,
) -> Result<Vec<Block>, DataSourceError> {
    let block_stream = stream::iter(from_block..=to_block)
        .map(|block_num| data_source.fetch_block_only(block_num));

    let mut buffered = block_stream.buffered(concurrency);
    let mut blocks = Vec::new();
    while let Some(result) = buffered.next().await {
        match result {
            Ok(block) => blocks.push(block),
            Err(e) => {
                tracing::error!(
                    error = %e,
                    from_block,
                    to_block,
                    "Failed to fetch block in range"
                );
                return Err(e);
            }
        }
    }
    Ok(blocks)
}

/// Fetches a range of blocks concurrently.
///
/// Uses a single `eth_getLogs(from, to)` call in parallel with all
/// `get_block_by_number` calls, replacing the previous per-block log-fetch
/// strategy. This collapses N log RTTs into one and overlaps it with block
/// fetching via `tokio::join!`.
///
/// Returns an error if any block fails to fetch. This ensures consistent
/// behavior across all components and prevents gaps in block processing.
pub async fn fetch_blocks_concurrent<D: DataSource + ?Sized>(
    data_source: &D,
    needs_receipts: bool,
    from_block: u64,
    to_block: u64,
    concurrency: usize,
) -> Result<Vec<BlockData>, DataSourceError> {
    // Fire the range log-fetch and all block-fetches in parallel.
    let (range_logs_result, blocks_result) = join!(
        data_source.fetch_logs_for_range(from_block, to_block),
        fetch_blocks_only(data_source, from_block, to_block, concurrency)
    );

    let range_logs = range_logs_result?;
    let blocks: Vec<Block> = blocks_result?;

    // Group logs by block number for O(1) lookup when building BlockData.
    let mut logs_by_block: HashMap<u64, Vec<_>> = HashMap::new();
    for log in range_logs {
        if let Some(block_num) = log.block_number {
            logs_by_block.entry(block_num).or_default().push(log);
        }
    }

    // Fetch all receipts in one batch if needed.
    let mut receipts_map = HashMap::new();
    if needs_receipts {
        let all_tx_hashes: Vec<_> = blocks.iter().flat_map(|b| b.transactions.hashes()).collect();
        if !all_tx_hashes.is_empty() {
            receipts_map = data_source.fetch_receipts(&all_tx_hashes).await?;
        }
    }

    // Combine blocks with their logs and receipts into BlockData.
    let mut block_data_vec: Vec<BlockData> = blocks
        .into_iter()
        .map(|block| {
            let block_num = block.header.number;
            let logs = logs_by_block.remove(&block_num).unwrap_or_default();
            let receipts = block
                .transactions
                .hashes()
                .filter_map(|h| receipts_map.remove(&h).map(|r| (h, r)))
                .collect();
            BlockData::from_raw_data(block, receipts, logs)
        })
        .collect();

    // Sort by block number to ensure correct order after concurrent fetching.
    block_data_vec.sort_by_key(|bd| bd.block.header.number);

    Ok(block_data_vec)
}
