use alloy::{
    primitives::{Address, B256, Bytes, LogData},
    rpc::types::Log as AlloyLog,
};

use crate::models::Log;

/// A builder for creating `Log` instances for testing.
#[derive(Debug, Clone, Default)]
pub struct LogBuilder {
    address: Address,
    topics: Vec<B256>,
    data: Bytes,
    transaction_hash: Option<B256>,
    transaction_index: Option<u64>,
    block_hash: Option<B256>,
    block_number: Option<u64>,
    log_index: Option<u64>,
    removed: bool,
}

impl LogBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn address(mut self, address: Address) -> Self {
        self.address = address;
        self
    }

    pub fn topic(mut self, topic: B256) -> Self {
        self.topics.push(topic);
        self
    }

    pub fn topics(mut self, topics: Vec<B256>) -> Self {
        self.topics = topics;
        self
    }

    pub fn data(mut self, data: Bytes) -> Self {
        self.data = data;
        self
    }

    pub fn transaction_hash(mut self, hash: B256) -> Self {
        self.transaction_hash = Some(hash);
        self
    }

    pub fn block_number(mut self, number: u64) -> Self {
        self.block_number = Some(number);
        self
    }

    pub fn log_index(mut self, index: u64) -> Self {
        self.log_index = Some(index);
        self
    }

    pub fn transaction_index(mut self, index: u64) -> Self {
        self.transaction_index = Some(index);
        self
    }

    pub fn build(self) -> Log {
        AlloyLog {
            inner: alloy::primitives::Log {
                address: self.address,
                data: LogData::new_unchecked(self.topics, self.data),
            },
            transaction_hash: self.transaction_hash,
            transaction_index: self.transaction_index,
            block_hash: self.block_hash,
            block_number: self.block_number,
            log_index: self.log_index,
            removed: self.removed,
            block_timestamp: None,
        }
        .into()
    }
}
