//! This module defines the data structure for a block's worth of correlated data.

use super::CorrelatedBlockItem;

/// Represents a block's worth of correlated data, ready for
/// filtering. This is the unit of data that will be passed from the
/// `BlockProcessor` to the `FilteringEngine`.
#[derive(Debug, Clone)]
pub struct CorrelatedBlockData {
    /// The block number.
    pub block_number: u64,
    /// The collection of correlated items (transactions, logs, etc.)
    /// from the block.
    pub items: Vec<CorrelatedBlockItem>,
}
