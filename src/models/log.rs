//! EVM log data structures.

use alloy::{
    primitives::{Address, B256, Bytes},
    rpc::types::Log as AlloyLog,
};
use serde::{Deserialize, Serialize};

/// A newtype wrapper around `alloy::rpc::types::Log` to create a stable
/// API boundary for the rest of the application.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Log(pub AlloyLog);

impl Log {
    /// Returns the address of the contract that emitted the log.
    pub fn address(&self) -> Address {
        self.0.address()
    }

    /// Returns the topics of the log.
    pub fn topics(&self) -> &[B256] {
        self.0.topics()
    }

    /// Returns the data of the log.
    pub fn data(&self) -> &Bytes {
        &self.0.data().data
    }

    /// Returns the hash of the block containing the log, or `None` if it's
    /// pending.
    pub fn block_hash(&self) -> Option<B256> {
        self.0.block_hash
    }

    /// Returns the number of the block containing the log, or `None` if it's
    /// pending.
    pub fn block_number(&self) -> Option<u64> {
        self.0.block_number
    }

    /// Returns the hash of the transaction that generated the log, or `None` if
    /// it's pending.
    pub fn transaction_hash(&self) -> Option<B256> {
        self.0.transaction_hash
    }

    /// Returns the index of the transaction that generated the log, or `None`
    /// if it's pending.
    pub fn transaction_index(&self) -> Option<u64> {
        self.0.transaction_index
    }

    /// Returns the index of the log in the block, or `None` if it's pending.
    pub fn log_index(&self) -> Option<u64> {
        self.0.log_index
    }

    /// Returns `true` if the log was removed due to a reorg.
    pub fn removed(&self) -> bool {
        self.0.removed
    }
}

/// The conversion from the alloy type to our custom type is a zero-cost move.
impl From<AlloyLog> for Log {
    fn from(log: AlloyLog) -> Self {
        Self(log)
    }
}

/// The conversion from our custom type back to the alloy type.
impl From<Log> for AlloyLog {
    fn from(log: Log) -> Self {
        log.0
    }
}
