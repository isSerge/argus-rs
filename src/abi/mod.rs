//! This module contains the `AbiService` for decoding and caching contract ABIs.

use alloy::{
    dyn_abi::DynSolValue,
    json_abi::{Event, Function, JsonAbi},
    primitives::{Address, B256},
    rpc::types::{Log, Transaction},
    sol_types,
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use thiserror::Error;

/// A pre-processed, cached representation of a contract's ABI.
///
/// This struct stores function and event definitions in hashmaps for fast,
/// O(1) lookups.
#[derive(Debug, Clone, Default)]
pub struct CachedContract {
    /// A map from a function's 4-byte selector to its `Function` definition.
    pub functions: HashMap<[u8; 4], Function>,
    /// A map from an event's 32-byte topic hash to its `Event` definition.
    pub events: HashMap<B256, Event>,
    /// The original parsed ABI.
    pub abi: JsonAbi,
}

impl From<&JsonAbi> for CachedContract {
    fn from(abi: &JsonAbi) -> Self {
        let functions = abi
            .functions()
            .map(|func| (func.selector(), func.clone()))
            .collect();

        let events = abi
            .events()
            .map(|event| (event.signature(), event.clone()))
            .collect();

        Self {
            functions,
            events,
            abi: abi.clone(),
        }
    }
}

/// Custom error type for the `AbiService`.
#[derive(Error, Debug)]
pub enum AbiError {
    #[error("Contract ABI not found for address: {0}")]
    AbiNotFound(Address),
    #[error("Event signature not found in ABI: {0}")]
    EventNotFound(B256),
    #[error("Function selector not found in ABI: {0:?}")]
    FunctionNotFound([u8; 4]),
    #[error("Input data too short to contain a function selector")]
    InputTooShort,
    #[error("Log has no topics, cannot identify event")]
    LogHasNoTopics,
    #[error("Failed to decode data: {0}")]
    DecodingError(#[from] sol_types::Error),
    #[error("Read lock was poisoned")]
    ReadLock,
    #[error("Write lock was poisoned")]
    WriteLock,
}

/// Represents a decoded event log.
#[derive(Debug)]
pub struct DecodedLog<'a> {
    /// The name of the decoded event.
    pub name: String,
    /// The decoded parameters of the event.
    pub params: Vec<(String, DynSolValue)>,
    /// A reference to the original log.
    pub log: &'a Log,
}

/// Represents a decoded function call.
#[derive(Debug)]
pub struct DecodedCall<'a> {
    /// The name of the decoded function.
    pub name: String,
    /// The decoded parameters of the function call.
    pub params: Vec<(String, DynSolValue)>,
    /// A reference to the original transaction.
    pub tx: &'a Transaction,
}

/// A service for managing and using contract ABIs.
///
/// This service caches parsed ABIs and provides methods for decoding
/// transaction data and event logs.
#[derive(Debug, Default)]
pub struct AbiService {
    cache: RwLock<HashMap<Address, Arc<CachedContract>>>,
}

impl AbiService {
    /// Creates a new `AbiService`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a contract's ABI to the service cache.
    ///
    /// The ABI is parsed and pre-processed for fast lookups.
    pub fn add_abi(&self, address: Address, abi: &JsonAbi) {
        let cached_contract = Arc::new(CachedContract::from(abi));
        let mut cache = self.cache.write().expect("RwLock is poisoned");
        cache.insert(address, cached_contract);
    }
}
