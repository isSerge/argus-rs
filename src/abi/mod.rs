//! This module contains the `AbiService` for decoding and caching contract
//! ABIs.
//!
//! It is designed to work with ABIs that are loaded at runtime, and therefore
//! does not use the `sol!` macro, which requires compile-time knowledge of the
//! ABI.
pub mod repository;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use alloy::{
    dyn_abi::{self, DynSolValue, EventExt},
    json_abi::{Event, Function, JsonAbi},
    primitives::{Address, B256},
};
use dashmap::DashMap;
use parking_lot::RwLock;
use thiserror::Error;

pub use self::repository::AbiRepository;
use crate::models::{Log, transaction::Transaction};

/// A pre-processed, cached representation of a contract's ABI.
///
/// This struct stores function and event definitions in hashmaps for fast,
/// O(1) lookups.
#[derive(Debug, Clone)]
pub struct CachedContract {
    /// A map from a function's 4-byte selector to its `Function` definition.
    pub functions: HashMap<[u8; 4], Function>,
    /// A map from an event's 32-byte topic hash to its `Event` definition.
    pub events: HashMap<B256, Event>,
    /// The original parsed ABI, shared via `Arc`.
    pub abi: Arc<JsonAbi>,
}

impl From<Arc<JsonAbi>> for CachedContract {
    fn from(abi: Arc<JsonAbi>) -> Self {
        // Clone Function and Event definitions to store owned copies in the HashMaps
        // for O(1) lookups.
        let functions = abi
            .functions()
            .map(|func| (func.selector().into(), func.clone()))
            .collect::<HashMap<[u8; 4], Function>>();

        let events = abi
            .events()
            .map(|event| (event.selector(), event.clone()))
            .collect::<HashMap<B256, Event>>();

        Self { functions, events, abi }
    }
}

impl PartialEq for CachedContract {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.abi, &other.abi)
    }
}
impl Eq for CachedContract {}

impl std::hash::Hash for CachedContract {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.abi).hash(state);
    }
}

/// Custom error type for the `AbiService`.
#[derive(Error, Debug)]
pub enum AbiError {
    /// Returned when no ABI is found for the given contract address
    #[error("Contract ABI not found for address: {0}")]
    AbiNotFound(Address),

    /// Returned when the specified ABI name is not found in the repository.
    #[error("ABI with name '{0}' not found in repository")]
    AbiNotFoundInRepository(String),

    /// Returned when an event signature (topic hash) is not found in the
    /// contract ABI
    #[error("Event signature not found in ABI: {0}")]
    EventNotFound(B256),

    /// Returned when a function selector (4-byte) is not found in the contract
    /// ABI
    #[error("Function selector not found in ABI: {0:?}")]
    FunctionNotFound([u8; 4]),

    /// Returned when trying to decode a function call from a contract creation
    /// transaction
    #[error("Transaction is a contract creation, not a function call")]
    ContractCreation,

    /// Returned when the input data is too short to contain a function selector
    #[error("Input data too short to contain a function selector")]
    InputTooShort,

    /// Returned when a log has no topics and thus cannot be identified as an
    /// event
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
#[derive(Debug)]
pub struct AbiService {
    /// Cache for pre-processed contract ABIs, mapped by contract address.
    address_specific_cache: DashMap<Address, Arc<CachedContract>>,
    /// A set of global ABIs used for decoding logs from any address.
    global_cache: RwLock<HashSet<Arc<CachedContract>>>,
    /// Repository for loading raw ABI definitions by name.
    abi_repository: Arc<AbiRepository>,
}

impl AbiService {
    /// Creates a new `AbiService`.
    pub fn new(abi_repository: Arc<AbiRepository>) -> Self {
        Self {
            address_specific_cache: DashMap::new(),
            global_cache: RwLock::new(HashSet::new()),
            abi_repository,
        }
    }

    /// Adds a global ABI to the service.
    pub fn add_global_abi(&self, abi_name: &str) -> Result<(), AbiError> {
        let abi = self
            .abi_repository
            .get_abi(abi_name)
            .ok_or_else(|| AbiError::AbiNotFoundInRepository(abi_name.to_string()))?;

        let cached_contract = Arc::new(CachedContract::from(abi));
        self.global_cache.write().insert(cached_contract);
        Ok(())
    }

    /// Links a contract address to an ABI by its name from the `AbiRepository`.
    /// The ABI is then parsed and pre-processed for fast lookups and cached.
    pub fn link_abi(&self, address: Address, abi_name: &str) -> Result<(), AbiError> {
        let abi = self
            .abi_repository
            .get_abi(abi_name)
            .ok_or_else(|| AbiError::AbiNotFoundInRepository(abi_name.to_string()))?;

        let cached_contract = Arc::new(CachedContract::from(abi));
        self.address_specific_cache.insert(address, cached_contract);
        Ok(())
    }

    /// Removes a contract's ABI from the service cache.
    ///
    /// Returns true if the ABI was present and removed, false if it wasn't in
    /// the cache.
    pub fn remove_abi(&self, address: &Address) -> bool {
        self.address_specific_cache.remove(address).is_some()
    }

    /// Checks if the service is configured to monitor a given address.
    pub fn is_monitored(&self, address: &Address) -> bool {
        self.address_specific_cache.contains_key(address)
    }

    /// Returns the number of ABIs in the cache.
    pub fn cache_size(&self) -> usize {
        self.address_specific_cache.len()
    }

    /// Decodes an event log using proper Alloy APIs.
    pub fn decode_log(&self, log: &Log) -> Result<DecodedLog, AbiError> {
        let event_signature = log.topics().first().ok_or(AbiError::LogHasNoTopics)?;

        // Prioritize address-specific ABIs.
        if let Some(contract) = self.address_specific_cache.get(&log.address())
            && contract.events.contains_key(event_signature)
        {
            return self.decode_log_with_contract(log, &contract, event_signature);
        }

        // Fallback to global ABIs if no address-specific match is found.
        for contract in self.global_cache.read().iter() {
            if contract.events.contains_key(event_signature) {
                return self.decode_log_with_contract(log, contract, event_signature);
            }
        }

        Err(AbiError::EventNotFound(*event_signature))
    }

    /// Decodes an event log using a specific cached contract ABI.
    fn decode_log_with_contract(
        &self,
        log: &Log,
        contract: &Arc<CachedContract>,
        event_signature: &B256,
    ) -> Result<DecodedLog, AbiError> {
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

        tracing::trace!("Decoded event: {} with {} parameters", event.name, params.len());

        Ok(DecodedLog { name: event.name.clone(), params, log: log.clone() })
    }

    /// Decodes a function call from transaction input data.
    ///
    /// This method extracts the function selector from the transaction input
    /// data, looks up the corresponding function definition, and decodes
    /// the parameters.
    pub fn decode_function_input(&self, tx: &Transaction) -> Result<DecodedCall, AbiError> {
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

        // Prioritize address-specific ABIs.
        if let Some(contract) = self.address_specific_cache.get(&to)
            && contract.functions.contains_key(&selector)
        {
            return self.decode_function_input_with_contract(tx, &contract, selector);
        }

        // Fallback to global ABIs if no address-specific match is found.
        for contract in self.global_cache.read().iter() {
            if contract.functions.contains_key(&selector) {
                return self.decode_function_input_with_contract(tx, contract, selector);
            }
        }

        Err(AbiError::FunctionNotFound(selector))
    }

    fn decode_function_input_with_contract(
        &self,
        tx: &Transaction,
        contract: &Arc<CachedContract>,
        selector: [u8; 4],
    ) -> Result<DecodedCall, AbiError> {
        let function = contract
            .functions
            .get(&selector)
            .ok_or_else(|| AbiError::FunctionNotFound(selector))?;

        let input_types: Vec<dyn_abi::DynSolType> =
            function.inputs.iter().map(|p| p.ty.parse()).collect::<Result<Vec<_>, _>>()?;

        let tuple_type = dyn_abi::DynSolType::Tuple(input_types);
        let decoded_value = tuple_type.abi_decode(&tx.input()[4..])?;

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

        Ok(DecodedCall { name: function.name.clone(), params, tx: tx.clone() })
    }

    /// Retrieves the cached ABI for a given contract address, if it exists.
    pub fn get_abi(&self, address: Address) -> Option<Arc<CachedContract>> {
        self.address_specific_cache.get(&address).map(|entry| Arc::clone(&entry))
    }

    /// Retrieves an ABI by its name from the `AbiRepository`, without linking
    /// it to a specific address. This is useful for retrieving ABIs for
    /// global log monitors (have no address).
    pub fn get_abi_by_name(&self, abi_name: &str) -> Option<Arc<JsonAbi>> {
        self.abi_repository.get_abi(abi_name)
    }

    /// Checks if a global ABI with the given name has been added to the
    /// service.
    pub fn has_global_abi(&self, abi_name: &str) -> bool {
        let global_cache = self.global_cache.read();
        global_cache.iter().any(|cached_contract| {
            self.abi_repository
                .get_abi(abi_name)
                .is_some_and(|repo_abi| Arc::ptr_eq(&repo_abi, &cached_contract.abi))
        })
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::{Address, Bytes, U256, address, b256, bytes};
    use tempfile::tempdir;

    use super::*;
    use crate::test_helpers::{
        LogBuilder, TransactionBuilder, create_test_abi_service, erc20_abi_json,
    };

    fn setup_abi_service_with_abi(abi_name: &str, abi_content: &str) -> (Arc<AbiService>, Address) {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[(abi_name, abi_content)]);
        let address = address!("0000000000000000000000000000000000000001");
        abi_service.link_abi(address, abi_name).unwrap();
        (abi_service, address)
    }

    #[test]
    fn test_link_abi_success() {
        let temp_dir = tempdir().unwrap();
        let (service, _) = create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json())]);
        let address = Address::default();

        assert!(!service.is_monitored(&address));
        assert_eq!(service.cache_size(), 0);

        let result = service.link_abi(address, "erc20");
        assert!(result.is_ok());
        assert!(service.is_monitored(&address));
        assert_eq!(service.cache_size(), 1);
    }

    #[test]
    fn test_link_abi_not_found_in_repository() {
        let temp_dir = tempdir().unwrap();
        let (service, _) = create_test_abi_service(&temp_dir, &[]); // Empty repo
        let address = Address::default();

        let result = service.link_abi(address, "nonexistent");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AbiError::AbiNotFoundInRepository(_)));
        assert!(!service.is_monitored(&address));
        assert_eq!(service.cache_size(), 0);
    }

    #[test]
    fn test_remove_abi() {
        let (service, address) = setup_abi_service_with_abi("erc20", erc20_abi_json());

        assert!(service.is_monitored(&address));
        assert_eq!(service.cache_size(), 1);

        assert!(service.remove_abi(&address));
        assert!(!service.is_monitored(&address));
        assert_eq!(service.cache_size(), 0);
    }

    #[test]
    fn test_decode_known_event() {
        let (service, contract_address) = setup_abi_service_with_abi("erc20", erc20_abi_json());

        let from = address!("1111111111111111111111111111111111111111");
        let to = address!("2222222222222222222222222222222222222222");
        let amount = U256::from(100);

        let log = LogBuilder::new()
            .address(contract_address)
            .topic(b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"))
            .topic(from.into_word())
            .topic(to.into_word())
            .data(bytes!("0000000000000000000000000000000000000000000000000000000000000064"))
            .build();

        let log: Log = log.into();
        let decoded = service.decode_log(&log).unwrap();
        assert_eq!(decoded.name, "Transfer");
        assert_eq!(decoded.params.len(), 3);
        assert_eq!(decoded.params[0].0, "from");
        assert_eq!(decoded.params[0].1, from.into());
        assert_eq!(decoded.params[1].0, "to");
        assert_eq!(decoded.params[1].1, to.into());
        assert_eq!(decoded.params[2].0, "value");
        assert_eq!(decoded.params[2].1, amount.into());
    }

    #[test]
    fn test_decode_known_function() {
        let (service, contract_address) = setup_abi_service_with_abi("erc20", erc20_abi_json());

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

        let decoded = service.decode_function_input(&tx).unwrap();
        assert_eq!(decoded.name, "transfer");
        assert_eq!(decoded.params.len(), 2);
        assert_eq!(decoded.params[0].0, "_to");
        assert_eq!(decoded.params[0].1, to_addr.into());
        assert_eq!(decoded.params[1].0, "_value");
        assert_eq!(decoded.params[1].1, amount.into());
    }

    #[test]
    fn test_decode_log_not_found() {
        let temp_dir = tempdir().unwrap();
        let (service, _) = create_test_abi_service(&temp_dir, &[]);

        let log = LogBuilder::new()
            .topic(b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"))
            .build();
        let log: Log = log.into();
        let err = service.decode_log(&log).unwrap_err();
        assert!(matches!(err, AbiError::EventNotFound(_)));
    }

    #[test]
    fn test_decode_log_global_abi() {
        let temp_dir = tempdir().unwrap();
        let (service, _) = create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json())]);
        service.add_global_abi("erc20").unwrap();

        let from = address!("1111111111111111111111111111111111111111");
        let to = address!("2222222222222222222222222222222222222222");

        let log = LogBuilder::new()
            .address(address!("3333333333333333333333333333333333333333")) // Some random address
            .topic(b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef")) // Transfer event signature
            .topic(from.into_word())
            .topic(to.into_word())
            .data(bytes!("0000000000000000000000000000000000000000000000000000000000000064"))
            .build();

        let decoded = service.decode_log(&log).unwrap();
        assert_eq!(decoded.name, "Transfer");
    }

    #[test]
    fn test_decode_function_not_found() {
        let temp_dir = tempdir().unwrap();
        let (service, _) = create_test_abi_service(&temp_dir, &[]);

        let tx = TransactionBuilder::new()
            .input(Bytes::from(vec![0x12, 0x34, 0x56, 0x78]))
            .to(Some(Address::default()))
            .build();
        let err = service.decode_function_input(&tx).unwrap_err();
        assert!(matches!(err, AbiError::FunctionNotFound(_)));
    }

    #[test]
    fn test_decode_function_for_unknown_selector() {
        let (service, contract_address) = setup_abi_service_with_abi("erc20", erc20_abi_json());

        let tx = TransactionBuilder::new()
            .to(Some(contract_address))
            .input(Bytes::from(vec![0x12, 0x34, 0x56, 0x78])) // Unknown selector
            .build();

        let err = service.decode_function_input(&tx).unwrap_err();
        assert!(matches!(err, AbiError::FunctionNotFound(_)));
    }

    #[test]
    fn test_decode_function_input_too_short() {
        let (service, contract_address) = setup_abi_service_with_abi("erc20", erc20_abi_json());

        // Default transaction has no input data
        let tx = TransactionBuilder::new().to(Some(contract_address)).build();

        let err = service.decode_function_input(&tx).unwrap_err();
        assert!(matches!(err, AbiError::InputTooShort));
    }

    #[test]
    fn test_decode_log_for_unknown_event() {
        let (service, contract_address) = setup_abi_service_with_abi("erc20", erc20_abi_json());

        let log = LogBuilder::new()
            .address(contract_address)
            .topic(b256!("0000000000000000000000000000000000000000000000000000000000000001"))
            .build();
        let err = service.decode_log(&log).unwrap_err();
        assert!(matches!(err, AbiError::EventNotFound(_)));
    }

    #[test]
    fn test_decode_log_with_no_topics() {
        let (service, contract_address) = setup_abi_service_with_abi("erc20", erc20_abi_json());
        let log = LogBuilder::new().address(contract_address).build();
        let err = service.decode_log(&log).unwrap_err();
        assert!(matches!(err, AbiError::LogHasNoTopics));
    }

    #[test]
    fn test_decode_contract_creation() {
        let temp_dir = tempdir().unwrap();
        let (service, _) = create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json())]);

        // Contract creation transactions have `to` as None.
        let tx = TransactionBuilder::new().to(None).build();

        let err = service.decode_function_input(&tx).unwrap_err();
        assert!(matches!(err, AbiError::ContractCreation));
    }

    #[test]
    fn test_decode_function_with_malformed_input() {
        let (service, contract_address) = setup_abi_service_with_abi("erc20", erc20_abi_json());

        // `transfer` selector, but the data is just a single byte.
        let input_data = bytes!("a9059cbb00").to_vec();

        let tx = TransactionBuilder::new()
            .to(Some(contract_address))
            .input(Bytes::from(input_data))
            .build();

        let err = service.decode_function_input(&tx).unwrap_err();
        assert!(matches!(err, AbiError::DecodingError(_)));
    }

    #[test]
    fn test_decode_log_with_malformed_data() {
        let (service, contract_address) = setup_abi_service_with_abi("erc20", erc20_abi_json());

        let from = address!("1111111111111111111111111111111111111111");
        let to = address!("2222222222222222222222222222222222222222");

        let log = LogBuilder::new()
            .address(contract_address)
            .topic(b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"))
            .topic(from.into_word())
            .topic(to.into_word())
            .data(bytes!("00000001"))
            .build();

        let err = service.decode_log(&log).unwrap_err();
        assert!(matches!(err, AbiError::DecodingError(_)));
    }

    #[test]
    fn test_get_abi() {
        let (service, contract_address) = setup_abi_service_with_abi("erc20", erc20_abi_json());

        let cached_contract = service.get_abi(contract_address).unwrap();
        assert_eq!(cached_contract.abi.functions().count(), 9); // ERC-20 has 9 functions
    }

    #[test]
    fn test_get_abi_by_name() {
        let temp_dir = tempdir().unwrap();
        let (service, _) = create_test_abi_service(&temp_dir, &[("test_abi", erc20_abi_json())]);

        // Test with an existing ABI name
        let abi = service.get_abi_by_name("test_abi").unwrap();
        assert_eq!(abi.functions().count(), 9); // ERC-20 has 9 functions

        // Test with a non-existent ABI name
        let non_existent_abi = service.get_abi_by_name("nonexistent");
        assert!(non_existent_abi.is_none());
    }

    #[test]
    fn test_decode_log_fallback_to_global() {
        let temp_dir = tempdir().unwrap();
        let (service, _) = create_test_abi_service(
            &temp_dir,
            &[
                (
                    "specific",
                    r#"[{"type": "event", "name": "SpecificEvent", "inputs": [], "anonymous": false}]"#,
                ),
                ("erc20", erc20_abi_json()),
            ],
        );

        let contract_address = address!("0000000000000000000000000000000000000001");
        service.link_abi(contract_address, "specific").unwrap();
        service.add_global_abi("erc20").unwrap();

        let from = address!("1111111111111111111111111111111111111111");
        let to = address!("2222222222222222222222222222222222222222");

        // This log has the "Transfer" event signature, which is in the global ABI,
        // but not in the address-specific ABI.
        let log = LogBuilder::new()
            .address(contract_address)
            .topic(b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"))
            .topic(from.into_word())
            .topic(to.into_word())
            .data(bytes!("0000000000000000000000000000000000000000000000000000000000000064"))
            .build();

        // The service should fail to decode with the specific ABI and fallback to the
        // global one.
        let decoded = service.decode_log(&log).unwrap();
        assert_eq!(decoded.name, "Transfer");
    }

    #[test]
    fn test_decode_log_global_abi_decoding_error() {
        let temp_dir = tempdir().unwrap();
        let (service, _) = create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json())]);
        service.add_global_abi("erc20").unwrap();

        // Log with correct "Transfer" signature but malformed data
        let log = LogBuilder::new()
            .address(address!("3333333333333333333333333333333333333333"))
            .topic(b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"))
            .data(bytes!("00000001")) // Malformed data
            .build();

        let err = service.decode_log(&log).unwrap_err();
        assert!(matches!(err, AbiError::DecodingError(_)));
    }

    #[test]
    fn test_decode_function_input_fallback_to_global() {
        let temp_dir = tempdir().unwrap();
        let (service, _) = create_test_abi_service(
            &temp_dir,
            &[
                (
                    "specific",
                    r#"[{"type": "function", "name": "specificFunction", "inputs": [], "outputs": []}]"#,
                ),
                ("erc20", erc20_abi_json()),
            ],
        );

        let contract_address = address!("0000000000000000000000000000000000000001");
        service.link_abi(contract_address, "specific").unwrap();
        service.add_global_abi("erc20").unwrap();

        let to_addr = address!("2222222222222222222222222222222222222222");
        let amount = U256::from(100);

        let mut input_data = bytes!("a9059cbb").to_vec(); // transfer function selector
        let to_addr_bytes = to_addr.as_slice();
        let mut padded_addr = [0u8; 32];
        padded_addr[12..].copy_from_slice(to_addr_bytes);
        input_data.extend_from_slice(&padded_addr);
        input_data.extend_from_slice(&amount.to_be_bytes_vec());

        // This transaction has the "transfer" function selector, which is in the global
        // ABI, but not in the address-specific ABI.
        let tx = TransactionBuilder::new()
            .to(Some(contract_address))
            .input(Bytes::from(input_data))
            .build();

        // The service should fail to decode with the specific ABI and fallback to the
        // global one.
        let decoded = service.decode_function_input(&tx).unwrap();
        assert_eq!(decoded.name, "transfer");
    }
}
