//! EVM transaction data structures.

use alloy::{
    consensus::Transaction as ConsensusTransaction,
    primitives::{Address, B256, Bytes, U256},
    rpc::types::Transaction as AlloyTransaction,
};
use serde::{Deserialize, Serialize};

/// A newtype wrapper around `alloy::rpc::types::Transaction` to create a stable
/// API boundary for the rest of the application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction(pub AlloyTransaction);

impl Transaction {
    /// Returns the transaction hash.
    pub fn hash(&self) -> B256 {
        *self.0.inner.hash()
    }

    /// Returns the recipient address, or `None` if it is a contract creation.
    pub fn to(&self) -> Option<Address> {
        self.0.inner.to()
    }

    /// Returns the sender address.
    pub fn from(&self) -> Address {
        self.0.inner.signer()
    }

    /// Returns the transaction input data.
    pub fn input(&self) -> &Bytes {
        self.0.inner.input()
    }

    /// Returns the value transferred in the transaction.
    pub fn value(&self) -> U256 {
        self.0.inner.value()
    }

    /// Returns the gas limit for the transaction.
    pub fn gas(&self) -> u64 {
        self.0.inner.gas_limit()
    }

    /// Returns the gas price for the transaction.
    pub fn gas_price(&self) -> Option<u128> {
        self.0.inner.gas_price()
    }

    /// Returns the transaction nonce.
    pub fn nonce(&self) -> u64 {
        self.0.inner.nonce()
    }
}

/// The conversion from the alloy type to our custom type is a zero-cost move.
impl From<AlloyTransaction> for Transaction {
    fn from(tx: AlloyTransaction) -> Self {
        Self(tx)
    }
}
