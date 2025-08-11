//! This module defines the `FilteringEngine` and its implementations.

use super::rhai::{
    bigint::register_bigint_with_rhai,
    conversions::{
        build_log_map, build_log_params_map, build_transaction_map, build_trigger_data_from_params,
    },
};
use crate::config::RhaiConfig;
use crate::models::correlated_data::CorrelatedBlockItem;
use crate::models::decoded_block::DecodedBlockData;
use crate::models::monitor::Monitor;
use crate::models::monitor_match::MonitorMatch;
use alloy::primitives::Address;
use async_trait::async_trait;
use dashmap::DashMap;
use futures::future;
#[cfg(test)]
use mockall::automock;
use rhai::{AST, Engine, EvalAltResult, Scope};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use thiserror::Error;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tokio::time::timeout;

/// Rhai script execution errors that can occur during compilation or runtime
#[derive(Debug, Error)]
pub enum RhaiError {
    /// Error that occurs during script compilation
    #[error("Script compilation failed: {0}")]
    CompilationError(#[from] Box<EvalAltResult>),

    /// Error that occurs during script runtime execution
    #[error("Script runtime error: {0}")]
    RuntimeError(Box<EvalAltResult>),

    /// Error that occurs when script execution exceeds the timeout limit
    #[error("Script execution timeout after {timeout:?}")]
    ExecutionTimeout {
        /// The timeout duration that was exceeded
        timeout: Duration,
    },
}

/// An error that occurs during monitor validation.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum MonitorValidationError {
    /// A monitor that accesses log data does not have a contract address specified.
    #[error(
        "Monitor '{monitor_name}' accesses log data ('log.*') but is not tied to a specific contract address. Please provide an 'address' for this monitor."
    )]
    MonitorRequiresAddress {
        /// The name of the monitor that failed validation.
        monitor_name: String,
    },

    /// A monitor that accesses log data does not have an ABI defined.
    #[error(
        "Monitor '{monitor_name}' accesses log data but does not have an ABI defined. Please provide an 'abi' file path."
    )]
    MonitorRequiresAbi {
        /// The name of the monitor that failed validation.
        monitor_name: String,
    },

    /// The address provided for a monitor is invalid.
    #[error("Invalid address for monitor '{monitor_name}': {address}")]
    InvalidAddress {
        /// The name of the monitor.
        monitor_name: String,
        /// The invalid address.
        address: String,
    },
}

/// A trait for an engine that applies filtering logic to block data.
#[cfg_attr(test, automock)]
#[async_trait]
pub trait FilteringEngine: Send + Sync {
    /// Evaluates the provided correlated block item against configured monitor rules.
    /// Returns a vector of `MonitorMatch` if any conditions are met.
    async fn evaluate_item(
        &self,
        item: &CorrelatedBlockItem,
    ) -> Result<Vec<MonitorMatch>, Box<dyn std::error::Error + Send + Sync>>;

    /// Updates the set of monitors used by the engine.
    async fn update_monitors(&self, monitors: Vec<Monitor>) -> Result<(), MonitorValidationError>;

    /// Returns true if any monitor requires transaction receipt data for evaluation.
    /// This allows optimizing data fetching by only including receipts when needed.
    fn requires_receipt_data(&self) -> bool;

    /// Runs the filtering engine with a stream of correlated block items.
    async fn run(&self, mut receiver: mpsc::Receiver<DecodedBlockData>);
}

/// A Rhai-based implementation of the `FilteringEngine` with integrated security controls.
#[derive(Debug)]
pub struct RhaiFilteringEngine {
    /// Monitors that are tied to a specific contract address.
    monitors_by_address: Arc<RwLock<DashMap<String, Vec<Monitor>>>>,
    /// Monitors that apply to all transactions.
    transaction_monitors: Arc<RwLock<Vec<Monitor>>>,
    engine: Engine,
    rhai_config: RhaiConfig,
    requires_receipts: AtomicBool,
}

impl RhaiFilteringEngine {
    /// Creates a new `RhaiFilteringEngine` with the given monitors and Rhai configuration.
    pub fn new(
        monitors: Vec<Monitor>,
        rhai_config: RhaiConfig,
    ) -> Result<Self, MonitorValidationError> {
        let mut engine = Engine::new();

        // Apply security limits
        engine.set_max_operations(rhai_config.max_operations);
        engine.set_max_call_levels(rhai_config.max_call_levels);
        engine.set_max_string_size(rhai_config.max_string_size);
        engine.set_max_array_size(rhai_config.max_array_size);

        // Disable dangerous language features
        Self::disable_dangerous_features(&mut engine);

        // Register BigInt wrapper for transparent big number handling
        register_bigint_with_rhai(&mut engine);

        let (monitors_by_address, transaction_monitors, needs_receipts) =
            Self::organize_monitors(monitors)?;

        Ok(Self {
            monitors_by_address: Arc::new(RwLock::new(monitors_by_address)),
            transaction_monitors: Arc::new(RwLock::new(transaction_monitors)),
            engine,
            rhai_config,
            requires_receipts: AtomicBool::new(needs_receipts),
        })
    }

    /// Organizes monitors into address-specific and transaction-level collections,
    /// and validates them.
    fn organize_monitors(
        monitors: Vec<Monitor>,
    ) -> Result<(DashMap<String, Vec<Monitor>>, Vec<Monitor>, bool), MonitorValidationError> {
        let monitors_by_address: DashMap<String, Vec<Monitor>> = DashMap::new();
        let mut transaction_monitors = Vec::new();
        let mut needs_receipts = false;

        for monitor in monitors {
            // Validate and categorize the monitor
            Self::validate_monitor(&monitor)?;

            if let Some(address_str) = &monitor.address {
                let address: Address =
                    address_str
                        .parse()
                        .map_err(|_| MonitorValidationError::InvalidAddress {
                            monitor_name: monitor.name.clone(),
                            address: address_str.clone(),
                        })?;

                let checksummed_address = address.to_checksum(None);

                monitors_by_address
                    .entry(checksummed_address)
                    .or_default()
                    .push(monitor.clone());
            } else {
                transaction_monitors.push(monitor.clone());
            }

            // Check if this monitor's script needs receipt data
            if !needs_receipts && Self::script_needs_receipt_data(&monitor.filter_script) {
                needs_receipts = true;
            }
        }

        Ok((monitors_by_address, transaction_monitors, needs_receipts))
    }

    /// Validates a single monitor configuration.
    fn validate_monitor(monitor: &Monitor) -> Result<(), MonitorValidationError> {
        if monitor.filter_script.contains("log.") {
            if monitor.address.is_none() {
                return Err(MonitorValidationError::MonitorRequiresAddress {
                    monitor_name: monitor.name.clone(),
                });
            }
            if monitor.abi.is_none() {
                return Err(MonitorValidationError::MonitorRequiresAbi {
                    monitor_name: monitor.name.clone(),
                });
            }
        }
        Ok(())
    }

    /// Disable dangerous language features and standard library functions
    fn disable_dangerous_features(engine: &mut Engine) {
        // List of dangerous symbols to disable
        const DANGEROUS_SYMBOLS: &[&str] = &[
            "eval", "import", "export", "print", "debug", "File", "file", "http", "net", "system",
            "process", "thread", "spawn",
        ];
        for &symbol in DANGEROUS_SYMBOLS {
            engine.disable_symbol(symbol);
        }
    }

    /// Compile a script with security checks
    fn compile_script(&self, script: &str) -> Result<AST, RhaiError> {
        self.engine
            .compile(script)
            .map_err(|e| RhaiError::CompilationError(e.into()))
    }

    /// Execute a pre-compiled AST with security controls including timeout
    async fn eval_ast_bool_secure(
        &self,
        ast: &AST,
        scope: &mut Scope<'_>,
    ) -> Result<bool, RhaiError> {
        // Execute with timeout protection
        let execution = async { self.engine.eval_ast_with_scope::<bool>(scope, ast) };

        match timeout(self.rhai_config.execution_timeout, execution).await {
            Ok(result) => result.map_err(RhaiError::RuntimeError),
            Err(_) => Err(RhaiError::ExecutionTimeout {
                timeout: self.rhai_config.execution_timeout,
            }),
        }
    }

    /// Analyzes a Rhai script to determine if it accesses receipt-related transaction fields.
    fn script_needs_receipt_data(script: &str) -> bool {
        // Receipt-specific fields that are only available from transaction receipts
        let receipt_fields = ["tx.gas_used", "tx.status", "tx.effective_gas_price"];

        // Simple string search for receipt-specific fields
        receipt_fields.iter().any(|field| script.contains(field))
    }
}

#[async_trait]
impl FilteringEngine for RhaiFilteringEngine {
    async fn run(&self, mut receiver: mpsc::Receiver<DecodedBlockData>) {
        while let Some(decoded_block) = receiver.recv().await {
            let futures = decoded_block
                .items
                .iter()
                .map(|item| self.evaluate_item(item));

            let results = future::join_all(futures).await;

            let mut all_matches = Vec::with_capacity(results.len());

            for result in results {
                match result {
                    Ok(matches) => all_matches.extend(matches),
                    Err(e) => {
                        tracing::error!("Error evaluating item: {}", e);
                        // Continue processing other items even if one fails
                    }
                }
            }

            if all_matches.is_empty() {
                tracing::debug!(
                    block_number = decoded_block.block_number,
                    "No matches found for block."
                );
            } else {
                tracing::info!(
                    block_number = decoded_block.block_number,
                    match_count = all_matches.len(),
                    "Found matches for block."
                );

                // TODO: send matches to notification queue
            }
        }
    }

    // TODO: add script compilation caching
    #[tracing::instrument(skip(self, item))]
    async fn evaluate_item(
        &self,
        item: &CorrelatedBlockItem,
    ) -> Result<Vec<MonitorMatch>, Box<dyn std::error::Error + Send + Sync>> {
        let mut matches = Vec::new();

        let tx_map = build_transaction_map(&item.transaction, item.receipt.as_ref());

        // --- Handle transaction-level monitors ---
        let transaction_monitors_guard = self.transaction_monitors.read().await;
        for monitor in transaction_monitors_guard.iter() {
            let mut scope = Scope::new();
            scope.push("tx", tx_map.clone());

            let ast = match self.compile_script(&monitor.filter_script) {
                Ok(ast) => ast,
                Err(e) => {
                    tracing::error!(monitor_id = monitor.id, "Failed to compile script: {}", e);
                    continue;
                }
            };

            match self.eval_ast_bool_secure(&ast, &mut scope).await {
                Ok(true) => {
                    let monitor_match = MonitorMatch {
                        monitor_id: monitor.id,
                        block_number: item.transaction.block_number().unwrap_or(0),
                        transaction_hash: item.transaction.hash(),
                        contract_address: Default::default(),
                        trigger_name: "transaction".to_string(),
                        trigger_data: Default::default(),
                        log_index: None,
                    };
                    matches.push(monitor_match);
                }
                Ok(false) => {
                    tracing::debug!(
                        monitor_id = monitor.id,
                        "Transaction monitor condition not met."
                    );
                }
                Err(e) => {
                    tracing::error!(monitor_id = monitor.id, "Script execution failed: {}", e);
                }
            }
        }
        drop(transaction_monitors_guard);

        // --- Handle log-based monitors ---
        let monitors_by_address_guard = self.monitors_by_address.read().await;
        if !monitors_by_address_guard.is_empty() {
            for log in &item.decoded_logs {
                let log_address_str = log.log.address().to_checksum(None);

                if let Some(monitors) = monitors_by_address_guard.get(&log_address_str) {
                    let params_map = build_log_params_map(&log.params);
                    let log_map = build_log_map(log, params_map);
                    let trigger_data = build_trigger_data_from_params(&log.params);

                    for monitor in monitors.iter() {
                        let mut scope = Scope::new();
                        scope.push("tx", tx_map.clone());
                        scope.push("log", log_map.clone());

                        let ast = match self.compile_script(&monitor.filter_script) {
                            Ok(ast) => ast,
                            Err(e) => {
                                tracing::error!(
                                    monitor_id = monitor.id,
                                    "Failed to compile script: {}",
                                    e
                                );
                                continue;
                            }
                        };

                        match self.eval_ast_bool_secure(&ast, &mut scope).await {
                            Ok(true) => {
                                let monitor_match = MonitorMatch {
                                    monitor_id: monitor.id,
                                    block_number: log.log.block_number().unwrap_or(0),
                                    transaction_hash: log
                                        .log
                                        .transaction_hash()
                                        .unwrap_or_default(),
                                    contract_address: log.log.address(),
                                    trigger_name: log.name.clone(),
                                    trigger_data: trigger_data.clone(),
                                    log_index: log.log.log_index(),
                                };
                                matches.push(monitor_match);
                            }
                            Ok(false) => {
                                tracing::debug!(
                                    monitor_id = monitor.id,
                                    "Log monitor condition not met."
                                );
                            }
                            Err(e) => {
                                tracing::error!(
                                    monitor_id = monitor.id,
                                    "Script execution failed: {}",
                                    e
                                );
                            }
                        }
                    }
                } else {
                    tracing::debug!("No monitors found for log address: {}", log_address_str);
                }
            }
        }

        Ok(matches)
    }

    /// Updates the set of monitors used by the engine.
    async fn update_monitors(&self, monitors: Vec<Monitor>) -> Result<(), MonitorValidationError> {
        let (new_monitors_by_address, new_transaction_monitors, needs_receipts) =
            Self::organize_monitors(monitors)?;

        let mut monitors_by_address_guard = self.monitors_by_address.write().await;
        *monitors_by_address_guard = new_monitors_by_address;
        drop(monitors_by_address_guard);

        let mut transaction_monitors_guard = self.transaction_monitors.write().await;
        *transaction_monitors_guard = new_transaction_monitors;
        drop(transaction_monitors_guard);

        self.requires_receipts
            .store(needs_receipts, Ordering::Relaxed);

        Ok(())
    }

    fn requires_receipt_data(&self) -> bool {
        self.requires_receipts.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::DecodedLog;
    use crate::config::RhaiConfig;
    use crate::models::transaction::Transaction;
    use crate::test_helpers::{LogBuilder, TransactionBuilder};
    use alloy::dyn_abi::DynSolValue;
    use alloy::primitives::{Address, U256, address};
    use serde_json::json;

    fn create_test_monitor(
        id: i64,
        address: Option<&str>,
        abi: Option<&str>,
        script: &str,
    ) -> Monitor {
        let mut monitor = Monitor::from_config(
            format!("Test Monitor {id}"),
            "testnet".to_string(),
            address.map(String::from),
            abi.map(String::from),
            script.to_string(),
        );
        monitor.id = id;
        monitor
    }

    fn create_test_log_and_tx(
        log_address: Address,
        log_name: &str,
        log_params: Vec<(String, DynSolValue)>,
    ) -> (Transaction, DecodedLog) {
        let tx = TransactionBuilder::new().build();
        let log_raw = LogBuilder::new().address(log_address).build();
        let log = DecodedLog {
            name: log_name.to_string(),
            params: log_params,
            log: log_raw.into(),
        };
        (tx.into(), log)
    }

    #[tokio::test]
    async fn test_new_and_update_monitors_organization() {
        let addr1_lower = "0x0000000000000000000000000000000000000001";
        let addr1_checksum = "0x0000000000000000000000000000000000000001";
        let addr2_lower = "0x0000000000000000000000000000000000000002";
        let addr2_checksum = "0x0000000000000000000000000000000000000002";

        let monitor1 = create_test_monitor(1, Some(addr1_lower), Some("abi.json"), "true");
        let monitor2 = create_test_monitor(2, Some(addr2_lower), Some("abi.json"), "true");
        let monitor3 = create_test_monitor(3, Some(addr1_lower), Some("abi.json"), "true");
        let monitor4 = create_test_monitor(4, None, None, "tx.value > 0"); // Tx monitor

        // Test `new()`
        let engine = RhaiFilteringEngine::new(
            vec![
                monitor1.clone(),
                monitor2.clone(),
                monitor3.clone(),
                monitor4.clone(),
            ],
            RhaiConfig::default(),
        )
        .unwrap();

        let monitors_by_address_read = engine.monitors_by_address.read().await;
        assert_eq!(monitors_by_address_read.len(), 2);
        assert_eq!(
            monitors_by_address_read.get(addr1_checksum).unwrap().len(),
            2
        );
        assert_eq!(
            monitors_by_address_read.get(addr2_checksum).unwrap().len(),
            1
        );
        drop(monitors_by_address_read);

        let transaction_monitors_read = engine.transaction_monitors.read().await;
        assert_eq!(transaction_monitors_read.len(), 1);
        assert_eq!(transaction_monitors_read[0].id, 4);
        drop(transaction_monitors_read);

        // Test `update_monitors()`
        let monitor5 = create_test_monitor(5, Some(addr2_lower), Some("abi.json"), "true");
        let monitor6 = create_test_monitor(6, None, None, "true");
        engine
            .update_monitors(vec![monitor1.clone(), monitor5.clone(), monitor6.clone()])
            .await
            .unwrap();

        let monitors_by_address_read = engine.monitors_by_address.read().await;
        assert_eq!(monitors_by_address_read.len(), 2);
        assert_eq!(
            monitors_by_address_read.get(addr1_checksum).unwrap().len(),
            1
        );
        assert_eq!(
            monitors_by_address_read.get(addr2_checksum).unwrap().len(),
            1
        );
        assert_eq!(
            monitors_by_address_read.get(addr2_checksum).unwrap()[0].id,
            5
        );
        drop(monitors_by_address_read);

        let transaction_monitors_read = engine.transaction_monitors.read().await;
        assert_eq!(transaction_monitors_read.len(), 1);
        assert_eq!(transaction_monitors_read[0].id, 6);
    }

    #[tokio::test]
    async fn test_evaluate_item_log_based_match() {
        let addr = address!("0000000000000000000000000000000000000001");
        let monitor = create_test_monitor(
            1,
            Some(&addr.to_checksum(None)),
            Some("abi.json"),
            "log.name == \"Transfer\"",
        );
        let engine = RhaiFilteringEngine::new(vec![monitor], RhaiConfig::default()).unwrap();

        let (tx, log) = create_test_log_and_tx(addr, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].trigger_name, "Transfer");
    }

    #[tokio::test]
    async fn test_evaluate_item_transaction_based_match() {
        let monitor = create_test_monitor(1, None, None, "bigint(tx.value) > bigint(100)");
        let engine = RhaiFilteringEngine::new(vec![monitor], RhaiConfig::default()).unwrap();

        let tx = TransactionBuilder::new().value(U256::from(150)).build();
        // This item has no logs, but should still be evaluated by the tx monitor
        let item = CorrelatedBlockItem::new(tx, vec![], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].trigger_name, "transaction");
    }

    #[tokio::test]
    async fn test_evaluate_item_no_match_for_tx_monitor() {
        let monitor = create_test_monitor(1, None, None, "bigint(tx.value) > bigint(200)");
        let engine = RhaiFilteringEngine::new(vec![monitor], RhaiConfig::default()).unwrap();

        let tx = TransactionBuilder::new().value(U256::from(150)).build();
        let item = CorrelatedBlockItem::new(tx, vec![], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert!(matches.is_empty());
    }

    #[tokio::test]
    async fn test_evaluate_item_mixed_monitors_both_match() {
        let addr = address!("0000000000000000000000000000000000000001");
        let log_monitor = create_test_monitor(
            1,
            Some(&addr.to_checksum(None)),
            Some("abi.json"),
            "log.name == \"Transfer\"",
        );
        let tx_monitor = create_test_monitor(2, None, None, "bigint(tx.value) > bigint(100)");
        let engine =
            RhaiFilteringEngine::new(vec![log_monitor, tx_monitor], RhaiConfig::default()).unwrap();

        // Create a transaction that will match the transaction-level monitor
        let tx = TransactionBuilder::new().value(U256::from(150)).build();

        // Create a log that will match the log-based monitor
        let log_raw = LogBuilder::new().address(addr).build();
        let log = DecodedLog {
            name: "Transfer".to_string(),
            params: vec![],
            log: log_raw.into(),
        };

        // The item contains both the transaction and the log
        let item = CorrelatedBlockItem::new(tx.into(), vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 2);
        let mut ids: Vec<i64> = matches.iter().map(|m| m.monitor_id).collect();
        ids.sort_unstable();
        assert_eq!(ids, vec![1, 2]);
    }

    #[tokio::test]
    async fn test_monitor_validation_success() {
        // Valid: log monitor accesses log and has address + ABI
        let monitor1 = create_test_monitor(
            1,
            Some("0x0000000000000000000000000000000000000123"),
            Some("abi.json"),
            "log.name == 'A'",
        );
        // Valid: tx monitor accesses tx
        let monitor2 = create_test_monitor(2, None, None, "tx.value > 0");
        // Valid: log monitor accesses tx only
        let monitor3 = create_test_monitor(
            3,
            Some("0x0000000000000000000000000000000000000123"),
            None,
            "tx.from == '0x456'",
        );

        assert!(
            RhaiFilteringEngine::new(vec![monitor1, monitor2, monitor3], RhaiConfig::default())
                .is_ok()
        );
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_requires_address() {
        // Invalid: accesses log data but has no address
        let invalid_monitor = create_test_monitor(1, None, None, "log.name == 'A'");
        let result = RhaiFilteringEngine::new(vec![invalid_monitor.clone()], RhaiConfig::default());

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            MonitorValidationError::MonitorRequiresAddress {
                monitor_name: invalid_monitor.name
            }
        );
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_requires_abi() {
        // Invalid: accesses log data but has no ABI
        let invalid_monitor = create_test_monitor(
            1,
            Some("0x0000000000000000000000000000000000000123"),
            None,
            "log.name == 'A'",
        );
        let result = RhaiFilteringEngine::new(vec![invalid_monitor.clone()], RhaiConfig::default());

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            MonitorValidationError::MonitorRequiresAbi {
                monitor_name: invalid_monitor.name
            }
        );
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_invalid_address() {
        // Invalid: address is not a valid hex string
        let invalid_addr = "not-a-valid-address";
        let invalid_monitor = create_test_monitor(1, Some(invalid_addr), Some("abi.json"), "true");
        let result = RhaiFilteringEngine::new(vec![invalid_monitor.clone()], RhaiConfig::default());

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            MonitorValidationError::InvalidAddress {
                monitor_name: invalid_monitor.name,
                address: invalid_addr.to_string()
            }
        );
    }

    #[tokio::test]
    async fn test_update_monitors_validation_failure() {
        let engine = RhaiFilteringEngine::new(vec![], RhaiConfig::default()).unwrap();
        // Invalid: accesses log data but has no address
        let invalid_monitor = create_test_monitor(1, None, None, "log.name == 'A'");

        let result = engine.update_monitors(vec![invalid_monitor.clone()]).await;

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            MonitorValidationError::MonitorRequiresAddress {
                monitor_name: invalid_monitor.name
            }
        );
    }

    #[tokio::test]
    async fn test_evaluate_item_filter_by_log_param() {
        let addr = address!("0000000000000000000000000000000000000001");
        let monitor = create_test_monitor(
            1,
            Some(&addr.to_checksum(None)),
            Some("abi.json"),
            "log.name == \"ValueTransfered\" && bigint(log.params.value) > bigint(100)",
        );
        let engine = RhaiFilteringEngine::new(vec![monitor], RhaiConfig::default()).unwrap();

        let (tx, log) = create_test_log_and_tx(
            addr,
            "ValueTransfered",
            vec![("value".to_string(), DynSolValue::Uint(U256::from(150), 256))],
        );
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].trigger_name, "ValueTransfered");
        assert_eq!(matches[0].trigger_data["value"], json!(150));
    }

    #[tokio::test]
    async fn test_evaluate_item_no_decoded_logs_still_triggers_tx_monitor() {
        let monitor = create_test_monitor(1, None, None, "true"); // Always matches
        let engine = RhaiFilteringEngine::new(vec![monitor], RhaiConfig::default()).unwrap();

        let tx = TransactionBuilder::new().build();
        let item = CorrelatedBlockItem::new(tx, vec![], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
    }
}
