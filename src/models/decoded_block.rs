//! This module defines the data structure for a block's worth of decoded data.

use super::CorrelatedBlockItem;

/// Represents a block's worth of decoded and correlated data, ready for filtering.
/// This is the unit of data that will be passed through the decoded data queue.
#[derive(Debug, Clone)]
pub struct DecodedBlockData {
    /// The block number.
    pub block_number: u64,
    /// The collection of correlated items (transactions, decoded logs, etc.) from the block.
    pub items: Vec<CorrelatedBlockItem>,
}
