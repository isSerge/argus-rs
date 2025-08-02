//! This module contains the `AbiService` for decoding and caching contract ABIs.
//!
//! It is designed to work with ABIs that are loaded at runtime, and therefore
//! does not use the `sol!` macro, which requires compile-time knowledge of the ABI.

use alloy::{
    consensus::Transaction as ConsensusTransaction,
    dyn_abi::{self, DynSolValue, EventExt, JsonAbiExt},
    json_abi::{Event, Function, JsonAbi},
    primitives::{Address, B256, TxKind},
    rpc::types::{Log, Transaction},
};
use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::Arc;
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
            .map(|func| (func.selector().into(), func.clone()))
            .collect::<HashMap<[u8; 4], Function>>();

        let events = abi
            .events()
            .map(|event| (event.selector(), event.clone()))
            .collect::<HashMap<B256, Event>>();

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
    /// Returned when no ABI is found for the given contract address
    #[error("Contract ABI not found for address: {0}")]
    AbiNotFound(Address),

    /// Returned when an event signature (topic hash) is not found in the contract ABI
    #[error("Event signature not found in ABI: {0}")]
    EventNotFound(B256),

    /// Returned when a function selector (4-byte) is not found in the contract ABI
    #[error("Function selector not found in ABI: {0:?}")]
    FunctionNotFound([u8; 4]),

    /// Returned when trying to decode a function call from a contract creation transaction
    #[error("Transaction is a contract creation, not a function call")]
    ContractCreation,

    /// Returned when the input data is too short to contain a function selector
    #[error("Input data too short to contain a function selector")]
    InputTooShort,

    /// Returned when a log has no topics and thus cannot be identified as an event
    #[error("Log has no topics, cannot identify event")]
    LogHasNoTopics,

    /// Wrapper for decoding errors from the underlying ABI decoding library
    #[error("Failed to decode data: {0}")]
    DecodingError(#[from] dyn_abi::Error),
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
/// transaction data and event logs. Uses `DashMap` for thread-safe
/// concurrent access without explicit locking.
#[derive(Debug, Default)]
pub struct AbiService {
    cache: DashMap<Address, Arc<CachedContract>>,
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
        self.cache.insert(address, cached_contract);
    }

    /// Removes a contract's ABI from the service cache.
    ///
    /// Returns true if the ABI was present and removed, false if it wasn't in the cache.
    pub fn remove_abi(&self, address: &Address) -> bool {
        self.cache.remove(address).is_some()
    }

    /// Checks if the cache contains an ABI for the given address.
    pub fn has_abi(&self, address: &Address) -> bool {
        self.cache.contains_key(address)
    }

    /// Returns the number of ABIs in the cache.
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }

    /// Decodes an event log using proper Alloy APIs.
    pub fn decode_log<'a>(&self, log: &'a Log) -> Result<DecodedLog<'a>, AbiError> {
        let contract = self
            .cache
            .get(&log.address())
            .ok_or_else(|| AbiError::AbiNotFound(log.address()))?;

        let event_signature = log.topics().first().ok_or(AbiError::LogHasNoTopics)?;

        let event = contract
            .events
            .get(event_signature)
            .ok_or_else(|| AbiError::EventNotFound(*event_signature))?;

        let decoded = event.decode_log_parts(log.topics().iter().copied(), log.data().data.as_ref())?;

        let params: Vec<(String, DynSolValue)> = event
            .inputs
            .iter()
            .zip(decoded.indexed.into_iter().chain(decoded.body.into_iter()))
            .map(|(input, value)| (input.name.clone(), value))
            .collect();

        tracing::trace!(
            "Decoded event: {} with {} parameters",
            event.name,
            params.len()
        );

        Ok(DecodedLog {
            name: event.name.clone(),
            params,
            log,
        })
    }

    /// Decodes a function call from transaction input data.
    ///
    /// This method extracts the function selector from the transaction input data,
    /// looks up the corresponding function definition, and decodes the parameters.
    pub fn decode_function_input<'a>(
        &self,
        tx: &'a Transaction,
    ) -> Result<DecodedCall<'a>, AbiError> {
        // Get the target contract address from the transaction
        let to = match tx.inner.kind() {
            TxKind::Call(to) => to,
            TxKind::Create => {
                tracing::debug!("Cannot decode function call from contract creation transaction");
                return Err(AbiError::ContractCreation);
            }
        };

        // Get the input data from the transaction
        let input = tx.inner.input();

        // The input data must be at least 4 bytes (the function selector)
        if input.len() < 4 {
            tracing::debug!("Transaction input too short: {} bytes", input.len());
            return Err(AbiError::InputTooShort);
        }

        // Extract the function selector (first 4 bytes)
        let selector: [u8; 4] = input[0..4].try_into().unwrap();

        // Look up the contract ABI in the cache
        let contract = self.cache.get(&to).ok_or_else(|| {
            tracing::debug!("No ABI found for contract address: {}", to);
            AbiError::AbiNotFound(to)
        })?;

        // Look up the function in the contract ABI using the selector
        let function = contract.functions.get(&selector).ok_or_else(|| {
            tracing::debug!("Function selector not found in ABI: {:?}", selector);
            AbiError::FunctionNotFound(selector)
        })?;

        let decoded_tokens = function.abi_decode_input(&input[4..])?;

        let params: Vec<(String, DynSolValue)> = function
            .inputs
            .iter()
            .zip(decoded_tokens)
            .map(|(input, token)| (input.name.clone(), token))
            .collect();

        tracing::trace!(
            "Decoded function call: {} with {} parameters",
            function.name,
            params.len()
        );

        Ok(DecodedCall {
            name: function.name.clone(),
            params,
            tx,
        })
    }
}
