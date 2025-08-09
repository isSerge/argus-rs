//! This module contains the `AbiService` for decoding and caching contract ABIs.
//!
//! It is designed to work with ABIs that are loaded at runtime, and therefore
//! does not use the `sol!` macro, which requires compile-time knowledge of the ABI.

use crate::models::{Log, transaction::Transaction};
use alloy::{
    dyn_abi::{self, DynSolValue, EventExt},
    json_abi::{Event, Function, JsonAbi},
    primitives::{Address, B256},
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
#[derive(Debug, Clone)]
pub struct DecodedLog {
    /// The name of the decoded event.
    pub name: String,
    /// The decoded parameters of the event.
    pub params: Vec<(String, DynSolValue)>,
    /// The original log
    pub log: Log,
}

/// Represents a decoded function call.
#[derive(Debug, Clone)]
pub struct DecodedCall {
    /// The name of the decoded function.
    pub name: String,
    /// The decoded parameters of the function call.
    pub params: Vec<(String, DynSolValue)>,
    /// The original transaction
    pub tx: Transaction,
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

    /// Checks if the service is configured to monitor a given address.
    pub fn is_monitored(&self, address: &Address) -> bool {
        self.cache.contains_key(address)
    }

    /// Returns the number of ABIs in the cache.
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }

    /// Decodes an event log using proper Alloy APIs.
    pub fn decode_log(&self, log: &Log) -> Result<DecodedLog, AbiError> {
        let contract = self
            .cache
            .get(&log.address())
            .ok_or_else(|| AbiError::AbiNotFound(log.address()))?;

        let event_signature = log.topics().first().ok_or(AbiError::LogHasNoTopics)?;

        let event = contract
            .events
            .get(event_signature)
            .ok_or_else(|| AbiError::EventNotFound(*event_signature))?;

        let decoded = event.decode_log_parts(log.topics().iter().copied(), log.data().as_ref())?;

        let params: Vec<(String, DynSolValue)> = event
            .inputs
            .iter()
            .zip(decoded.indexed.into_iter().chain(decoded.body))
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
            log: log.clone(),
        })
    }

    /// Decodes a function call from transaction input data.
    ///
    /// This method extracts the function selector from the transaction input data,
    /// looks up the corresponding function definition, and decodes the parameters.
    pub fn decode_function_input(&self, tx: Transaction) -> Result<DecodedCall, AbiError> {
        // Get the target contract address from the transaction

        let to = tx.to().ok_or_else(|| {
            tracing::debug!("Cannot decode function call from contract creation transaction");
            AbiError::ContractCreation
        })?;

        // Get the input data from the transaction
        let input = tx.input();

        // The input data must be at least 4 bytes (the function selector)
        if input.len() < 4 {
            tracing::debug!("Transaction input too short: {} bytes", input.len());
            return Err(AbiError::InputTooShort);
        }

        // Extract the function selector (first 4 bytes)
        let selector: [u8; 4] = input[0..4].try_into().unwrap();

        let contract = self
            .cache
            .get(&to)
            .ok_or_else(|| AbiError::AbiNotFound(to))?;

        // Look up the function in the contract ABI using the selector
        let function = contract
            .functions
            .get(&selector)
            .ok_or_else(|| AbiError::FunctionNotFound(selector))?;

        let input_types: Vec<dyn_abi::DynSolType> = function
            .inputs
            .iter()
            .map(|p| p.ty.parse())
            .collect::<Result<Vec<_>, _>>()?;

        let tuple_type = dyn_abi::DynSolType::Tuple(input_types);
        let decoded_value = tuple_type.abi_decode(&input[4..])?;

        let decoded_tokens = if let DynSolValue::Tuple(tokens) = decoded_value {
            tokens
        } else {
            return Err(AbiError::DecodingError(dyn_abi::Error::TypeMismatch {
                expected: tuple_type.to_string(),
                actual: format!("{decoded_value:?}"),
            }));
        };

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{LogBuilder, TransactionBuilder};
    use alloy::{
        primitives::{Address, Bytes, U256, address, b256, bytes},
        rpc::types::Log as AlloyLog,
    };

    fn simple_abi() -> JsonAbi {
        serde_json::from_str(
            r#"[
                {
                    "type": "function",
                    "name": "transfer",
                    "inputs": [
                        {"name": "to", "type": "address"},
                        {"name": "amount", "type": "uint256"}
                    ],
                    "outputs": [{"name": "success", "type": "bool"}]
                },
                {
                    "type": "event",
                    "name": "Transfer",
                    "inputs": [
                        {"name": "from", "type": "address", "indexed": true},
                        {"name": "to", "type": "address", "indexed": true},
                        {"name": "amount", "type": "uint256", "indexed": false}
                    ],
                    "anonymous": false
                }
            ]"#,
        )
        .unwrap()
    }

    #[test]
    fn test_add_and_remove_abi() {
        let service = AbiService::new();
        let abi = simple_abi();
        let address = Address::default();

        assert!(!service.is_monitored(&address));
        assert_eq!(service.cache_size(), 0);

        service.add_abi(address, &abi);
        assert!(service.is_monitored(&address));
        assert_eq!(service.cache_size(), 1);

        assert!(service.remove_abi(&address));
        assert!(!service.is_monitored(&address));
        assert_eq!(service.cache_size(), 0);
    }

    #[test]
    fn test_decode_known_event() {
        let service = AbiService::new();
        let abi = simple_abi();
        let contract_address = address!("0000000000000000000000000000000000000001");
        service.add_abi(contract_address, &abi);

        let from = address!("1111111111111111111111111111111111111111");
        let to = address!("2222222222222222222222222222222222222222");
        let amount = U256::from(100);

        let log = LogBuilder::new()
            .address(contract_address)
            .topic(b256!(
                "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
            ))
            .topic(from.into_word())
            .topic(to.into_word())
            .data(bytes!(
                "0000000000000000000000000000000000000000000000000000000000000064"
            ))
            .build();

        let log: Log = log.into();
        let decoded = service.decode_log(&log).unwrap();
        assert_eq!(decoded.name, "Transfer");
        assert_eq!(decoded.params.len(), 3);
        assert_eq!(decoded.params[0].0, "from");
        assert_eq!(decoded.params[0].1, from.into());
        assert_eq!(decoded.params[1].0, "to");
        assert_eq!(decoded.params[1].1, to.into());
        assert_eq!(decoded.params[2].0, "amount");
        assert_eq!(decoded.params[2].1, amount.into());
    }

    #[test]
    fn test_decode_known_function() {
        let service = AbiService::new();
        let abi = simple_abi();
        let contract_address = address!("0000000000000000000000000000000000000001");
        service.add_abi(contract_address, &abi);

        let to_addr = address!("2222222222222222222222222222222222222222");
        let amount = U256::from(100);

        let mut input_data = bytes!("a9059cbb").to_vec();
        let to_addr_bytes = to_addr.as_slice(); // Returns &[u8] of length 20
        let mut padded_addr = [0u8; 32];
        padded_addr[12..].copy_from_slice(to_addr_bytes); // Copy address to last 20 bytes
        input_data.extend_from_slice(&padded_addr);
        input_data.extend_from_slice(&amount.to_be_bytes_vec());

        let tx = TransactionBuilder::new()
            .to(Some(contract_address))
            .input(Bytes::from(input_data))
            .build();

        let decoded = service.decode_function_input(tx).unwrap();
        assert_eq!(decoded.name, "transfer");
        assert_eq!(decoded.params.len(), 2);
        assert_eq!(decoded.params[0].0, "to");
        assert_eq!(decoded.params[0].1, to_addr.into());
        assert_eq!(decoded.params[1].0, "amount");
        assert_eq!(decoded.params[1].1, amount.into());
    }

    #[test]
    fn test_decode_log_not_found() {
        let service = AbiService::new();
        let log = AlloyLog::default();
        let log = log.into();
        let err = service.decode_log(&log).unwrap_err();
        assert!(matches!(err, AbiError::AbiNotFound(_)));
    }

    #[test]
    fn test_decode_function_not_found() {
        let service = AbiService::new();
        let tx = TransactionBuilder::new()
            .input(Bytes::from(vec![0u8; 32]))
            .to(Some(Address::default()))
            .build();
        let err = service.decode_function_input(tx).unwrap_err();
        assert!(matches!(err, AbiError::AbiNotFound(_)));
    }

    #[test]
    fn test_decode_function_for_unknown_selector() {
        let service = AbiService::new();
        let abi = simple_abi();
        let contract_address = address!("0000000000000000000000000000000000000001");
        service.add_abi(contract_address, &abi);

        let tx = TransactionBuilder::new()
            .to(Some(contract_address))
            .input(Bytes::from(vec![0x12, 0x34, 0x56, 0x78])) // Unknown selector
            .build();

        let err = service.decode_function_input(tx).unwrap_err();
        assert!(matches!(err, AbiError::FunctionNotFound(_)));
    }

    #[test]
    fn test_decode_function_input_too_short() {
        let service = AbiService::new();
        let abi = simple_abi();
        let contract_address = address!("0000000000000000000000000000000000000001");
        service.add_abi(contract_address, &abi);

        // Default transaction has no input data
        let tx = TransactionBuilder::new().to(Some(contract_address)).build();

        let err = service.decode_function_input(tx).unwrap_err();
        assert!(matches!(err, AbiError::InputTooShort));
    }

    #[test]
    fn test_decode_log_for_unknown_event() {
        let service = AbiService::new();
        let abi = simple_abi();
        let contract_address = address!("0000000000000000000000000000000000000001");
        service.add_abi(contract_address, &abi);

        let log = LogBuilder::new()
            .address(contract_address)
            .topic(b256!(
                "0000000000000000000000000000000000000000000000000000000000000001"
            ))
            .build();
        let err = service.decode_log(&log).unwrap_err();
        assert!(matches!(err, AbiError::EventNotFound(_)));
    }

    #[test]
    fn test_decode_log_with_no_topics() {
        let service = AbiService::new();
        let abi = simple_abi();
        let contract_address = address!("0000000000000000000000000000000000000001");
        service.add_abi(contract_address, &abi);
        let log = LogBuilder::new().address(contract_address).build();
        let err = service.decode_log(&log).unwrap_err();
        assert!(matches!(err, AbiError::LogHasNoTopics));
    }

    #[test]
    fn test_decode_contract_creation() {
        let service = AbiService::new();
        // Contract creation transactions have `to` as None.
        let tx = TransactionBuilder::new().to(None).build();

        let err = service.decode_function_input(tx).unwrap_err();
        assert!(matches!(err, AbiError::ContractCreation));
    }

    #[test]
    fn test_decode_function_with_malformed_input() {
        let service = AbiService::new();
        let abi = simple_abi();
        let contract_address = address!("0000000000000000000000000000000000000001");
        service.add_abi(contract_address, &abi);

        // `transfer` selector, but the data is just a single byte.
        let input_data = bytes!("a9059cbb00").to_vec();

        let tx = TransactionBuilder::new()
            .to(Some(contract_address))
            .input(Bytes::from(input_data))
            .build();

        let err = service.decode_function_input(tx).unwrap_err();
        assert!(matches!(err, AbiError::DecodingError(_)));
    }

    #[test]
    fn test_decode_log_with_malformed_data() {
        let service = AbiService::new();
        let abi = simple_abi();
        let contract_address = address!("0000000000000000000000000000000000000001");
        service.add_abi(contract_address, &abi);

        let from = address!("1111111111111111111111111111111111111111");
        let to = address!("2222222222222222222222222222222222222222");

        let log = LogBuilder::new()
            .address(contract_address)
            .topic(b256!(
                "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
            ))
            .topic(from.into_word())
            .topic(to.into_word())
            .data(bytes!("00000001"))
            .build();

        let err = service.decode_log(&log).unwrap_err();
        assert!(matches!(err, AbiError::DecodingError(_)));
    }
}
