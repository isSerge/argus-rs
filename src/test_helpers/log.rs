//! A builder for creating `Log` instances for testing.

use crate::models::Log;
use alloy::{
    primitives::{Address, B256, Bytes, LogData},
    rpc::types::Log as AlloyLog,
};

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
    /// Creates a new `LogBuilder`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the address of the contract that emitted the log.
    pub fn address(mut self, address: Address) -> Self {
        self.address = address;
        self
    }

    /// Adds a topic to the log.
    pub fn topic(mut self, topic: B256) -> Self {
        self.topics.push(topic);
        self
    }

    /// Sets the topics of the log.
    pub fn topics(mut self, topics: Vec<B256>) -> Self {
        self.topics = topics;
        self
    }

    /// Sets the data of the log.
    pub fn data(mut self, data: Bytes) -> Self {
        self.data = data;
        self
    }

    /// Sets the transaction hash of the log.
    pub fn transaction_hash(mut self, hash: B256) -> Self {
        self.transaction_hash = Some(hash);
        self
    }

    /// Sets the transaction index of the log.
    pub fn transaction_index(mut self, index: u64) -> Self {
        self.transaction_index = Some(index);
        self
    }

    /// Sets the block hash of the log.
    pub fn block_hash(mut self, hash: B256) -> Self {
        self.block_hash = Some(hash);
        self
    }

    /// Sets the block number of the log.
    pub fn block_number(mut self, number: u64) -> Self {
        self.block_number = Some(number);
        self
    }

    /// Sets the log index of the log.
    pub fn log_index(mut self, index: u64) -> Self {
        self.log_index = Some(index);
        self
    }

    /// Sets the removed status of the log.
    pub fn removed(mut self, removed: bool) -> Self {
        self.removed = removed;
        self
    }

    /// Builds the `AlloyLog` with the provided values.
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

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::{address, b256};

    #[test]
    fn test_log_builder() {
        let log = LogBuilder::new()
            .address(address!("1111111111111111111111111111111111111111"))
            .topic(b256!(
                "2222222222222222222222222222222222222222222222222222222222222222"
            ))
            .data(Bytes::from(vec![0xde, 0xad, 0xbe, 0xef]))
            .build();

        assert_eq!(
            log.address(),
            address!("1111111111111111111111111111111111111111")
        );
        assert_eq!(log.topics().len(), 1);
        assert_eq!(
            log.topics()[0],
            b256!("2222222222222222222222222222222222222222222222222222222222222222")
        );
        assert_eq!(*log.data(), Bytes::from(vec![0xde, 0xad, 0xbe, 0xef]));
    }
}
