//! This module defines the `FilteringEngine` and its implementations.

use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use alloy::primitives::Address;
use async_trait::async_trait;
use dashmap::DashMap;
use futures::future;
#[cfg(test)]
use mockall::automock;
use rhai::{AST, Engine, EvalAltResult, Scope};
use thiserror::Error;
use tokio::{
    sync::{RwLock, mpsc},
    time::timeout,
};

use super::rhai::{
    conversions::{
        build_log_map, build_log_params_map, build_transaction_map, build_trigger_data_from_params,
        build_trigger_data_from_transaction,
    },
    create_engine,
};
use crate::{
    config::RhaiConfig,
    engine::rhai::compiler::{RhaiCompiler, RhaiCompilerError},
    models::{
        correlated_data::CorrelatedBlockItem, decoded_block::DecodedBlockData, monitor::Monitor,
        monitor_match::MonitorMatch,
    },
};

/// Rhai script execution errors that can occur during compilation or runtime
#[derive(Debug, Error)]
pub enum RhaiError {
    /// Error that occurs during script compilation
    #[error("Script compilation failed: {0}")]
    CompilationError(#[from] RhaiCompilerError),

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

/// A trait for an engine that applies filtering logic to block data.
#[cfg_attr(test, automock)]
#[async_trait]
pub trait FilteringEngine: Send + Sync {
    /// Evaluates the provided correlated block item against configured monitor
    /// rules. Returns a vector of `MonitorMatch` if any conditions are met.
    async fn evaluate_item(
        &self,
        item: &CorrelatedBlockItem,
    ) -> Result<Vec<MonitorMatch>, RhaiError>;

    /// Updates the set of monitors used by the engine.
    async fn update_monitors(&self, monitors: Vec<Monitor>);

    /// Returns true if any monitor requires transaction receipt data for
    /// evaluation. This allows optimizing data fetching by only including
    /// receipts when needed.
    fn requires_receipt_data(&self) -> bool;

    /// Runs the filtering engine with a stream of correlated block items and
    /// sends matches to the notification queue.
    async fn run(
        &self,
        mut receiver: mpsc::Receiver<DecodedBlockData>,
        notifications_tx: mpsc::Sender<MonitorMatch>,
    );
}

/// A Rhai-based implementation of the `FilteringEngine` with integrated
/// security controls.
#[derive(Debug)]
pub struct RhaiFilteringEngine {
    /// Monitors that are tied to a specific contract address.
    monitors_by_address: Arc<RwLock<DashMap<String, Vec<Monitor>>>>,
    /// Monitors that apply to all transactions.
    transaction_monitors: Arc<RwLock<Vec<Monitor>>>,
    compiler: Arc<RhaiCompiler>,
    requires_receipts: AtomicBool,
    config: RhaiConfig,
    engine: Engine,
}

impl RhaiFilteringEngine {
    /// Creates a new `RhaiFilteringEngine` with the given monitors and Rhai
    /// configuration.
    pub fn new(monitors: Vec<Monitor>, compiler: Arc<RhaiCompiler>, config: RhaiConfig) -> Self {
        // Currently use the default Rhai engine creation function, but can consider
        // adding more customizations later
        let engine = create_engine(config.clone());

        let (monitors_by_address, transaction_monitors, needs_receipts) =
            Self::organize_monitors(monitors, &compiler);

        Self {
            monitors_by_address: Arc::new(RwLock::new(monitors_by_address)),
            transaction_monitors: Arc::new(RwLock::new(transaction_monitors)),
            compiler,
            requires_receipts: AtomicBool::new(needs_receipts),
            config,
            engine,
        }
    }

    /// Organizes monitors into address-specific and transaction-level
    /// collections, and validates them.
    fn organize_monitors(
        monitors: Vec<Monitor>,
        compiler: &RhaiCompiler,
    ) -> (DashMap<String, Vec<Monitor>>, Vec<Monitor>, bool) {
        let monitors_by_address: DashMap<String, Vec<Monitor>> = DashMap::new();
        let mut transaction_monitors = Vec::new();
        let mut needs_receipts = false;

        // Receipt-specific fields that are only available from transaction receipts
        let receipt_fields: HashSet<String> = [
            "tx.gas_used".to_string(),
            "tx.status".to_string(),
            "tx.effective_gas_price".to_string(),
        ]
        .into_iter()
        .collect();

        for monitor in monitors {
            if let Some(address_str) = &monitor.address {
                let address: Address =
                    address_str.parse().expect("Address should be valid at this stage");

                let checksummed_address = address.to_checksum(None);

                monitors_by_address.entry(checksummed_address).or_default().push(monitor.clone());
            } else {
                transaction_monitors.push(monitor.clone());
            }

            // Check if this monitor's script needs receipt data
            if !needs_receipts {
                match compiler.analyze_script(&monitor.filter_script) {
                    Ok(analysis) => {
                        // Check for intersection between accessed variables and receipt fields.
                        if !analysis.accessed_variables.is_disjoint(&receipt_fields) {
                            needs_receipts = true;
                        }
                    }
                    Err(e) => {
                        // If a script fails to compile, we can't analyze it. Log an error.
                        // It won't match anyway, but this highlights a problem.
                        tracing::error!(monitor_id = monitor.id, error = ?e, "Failed to compile and analyze script during monitor organization");
                    }
                }
            }
        }

        (monitors_by_address, transaction_monitors, needs_receipts)
    }

    /// Execute a pre-compiled AST with security controls including timeout
    async fn eval_ast_bool_secure(
        &self,
        ast: &AST,
        scope: &mut Scope<'_>,
    ) -> Result<bool, RhaiError> {
        // Execute with timeout protection
        let execution = async { self.engine.eval_ast_with_scope::<bool>(scope, ast) };

        match timeout(self.config.execution_timeout, execution).await {
            Ok(result) => result.map_err(RhaiError::RuntimeError),
            Err(_) => Err(RhaiError::ExecutionTimeout { timeout: self.config.execution_timeout }),
        }
    }
}

#[async_trait]
impl FilteringEngine for RhaiFilteringEngine {
    async fn run(
        &self,
        mut receiver: mpsc::Receiver<DecodedBlockData>,
        notifications_tx: mpsc::Sender<MonitorMatch>,
    ) {
        while let Some(decoded_block) = receiver.recv().await {
            let futures = decoded_block.items.iter().map(|item| self.evaluate_item(item));

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

                // Send all matches to the notification channel
                for monitor_match in all_matches {
                    if let Err(e) = notifications_tx.send(monitor_match).await {
                        tracing::error!("Failed to send notification match: {}", e);
                    }
                }
            }
        }
    }

    // TODO: add script compilation caching
    #[tracing::instrument(skip(self, item))]
    async fn evaluate_item(
        &self,
        item: &CorrelatedBlockItem,
    ) -> Result<Vec<MonitorMatch>, RhaiError> {
        let mut matches = Vec::new();

        let tx_map = build_transaction_map(&item.transaction, item.receipt.as_ref());

        // --- Handle transaction-level monitors ---
        let transaction_monitors_guard = self.transaction_monitors.read().await;
        for monitor in transaction_monitors_guard.iter() {
            let mut scope = Scope::new();
            scope.push("tx", tx_map.clone());

            let ast = self.compiler.get_ast(&monitor.filter_script)?;

            match self.eval_ast_bool_secure(&ast, &mut scope).await {
                Ok(true) => {
                    tracing::debug!(
                        monitor_id = monitor.id,
                        tx_hash = %item.transaction.hash(),
                        "Transaction monitor condition met."
                    );
                    let trigger_data = build_trigger_data_from_transaction(
                        &item.transaction,
                        item.receipt.as_ref(),
                    );
                    let monitor_match = MonitorMatch {
                        monitor_id: monitor.id,
                        block_number: item.transaction.block_number().unwrap_or(0),
                        transaction_hash: item.transaction.hash(),
                        contract_address: Default::default(),
                        notifier_name: "transaction".to_string(),
                        trigger_data,
                        log_index: None,
                    };
                    matches.push(monitor_match);
                }
                Ok(false) => {
                    tracing::debug!(
                        monitor_id = monitor.id,
                        tx_hash = %item.transaction.hash(),
                        "Transaction monitor condition NOT met."
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

                        let ast = self.compiler.get_ast(&monitor.filter_script)?;

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
                                    notifier_name: log.name.clone(),
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
    async fn update_monitors(&self, monitors: Vec<Monitor>) {
        let (new_monitors_by_address, new_transaction_monitors, needs_receipts) =
            Self::organize_monitors(monitors, self.compiler.as_ref());

        let mut monitors_by_address_guard = self.monitors_by_address.write().await;
        *monitors_by_address_guard = new_monitors_by_address;
        drop(monitors_by_address_guard);

        let mut transaction_monitors_guard = self.transaction_monitors.write().await;
        *transaction_monitors_guard = new_transaction_monitors;
        drop(transaction_monitors_guard);

        self.requires_receipts.store(needs_receipts, Ordering::Relaxed);
    }

    fn requires_receipt_data(&self) -> bool {
        self.requires_receipts.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use alloy::{
        dyn_abi::DynSolValue,
        primitives::{Address, U256, address},
    };
    use serde_json::json;

    use super::*;
    use crate::{
        abi::DecodedLog,
        config::RhaiConfig,
        models::transaction::Transaction,
        test_helpers::{LogBuilder, TransactionBuilder},
    };

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
        let log =
            DecodedLog { name: log_name.to_string(), params: log_params, log: log_raw.into() };
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
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config.clone()));
        let engine = RhaiFilteringEngine::new(
            vec![monitor1.clone(), monitor2.clone(), monitor3.clone(), monitor4.clone()],
            compiler,
            config,
        );

        let monitors_by_address_read = engine.monitors_by_address.read().await;
        assert_eq!(monitors_by_address_read.len(), 2);
        assert_eq!(monitors_by_address_read.get(addr1_checksum).unwrap().len(), 2);
        assert_eq!(monitors_by_address_read.get(addr2_checksum).unwrap().len(), 1);
        drop(monitors_by_address_read);

        let transaction_monitors_read = engine.transaction_monitors.read().await;
        assert_eq!(transaction_monitors_read.len(), 1);
        assert_eq!(transaction_monitors_read[0].id, 4);
        drop(transaction_monitors_read);

        // Test `update_monitors()`
        let monitor5 = create_test_monitor(5, Some(addr2_lower), Some("abi.json"), "true");
        let monitor6 = create_test_monitor(6, None, None, "true");
        engine.update_monitors(vec![monitor1.clone(), monitor5.clone(), monitor6.clone()]).await;

        let monitors_by_address_read = engine.monitors_by_address.read().await;
        assert_eq!(monitors_by_address_read.len(), 2);
        assert_eq!(monitors_by_address_read.get(addr1_checksum).unwrap().len(), 1);
        assert_eq!(monitors_by_address_read.get(addr2_checksum).unwrap().len(), 1);
        assert_eq!(monitors_by_address_read.get(addr2_checksum).unwrap()[0].id, 5);
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
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config.clone()));
        let engine = RhaiFilteringEngine::new(vec![monitor], compiler, config);

        let (tx, log) = create_test_log_and_tx(addr, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].notifier_name, "Transfer");
    }

    #[tokio::test]
    async fn test_evaluate_item_transaction_based_match() {
        let monitor = create_test_monitor(1, None, None, "bigint(tx.value) > bigint(100)");
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config.clone()));
        let engine = RhaiFilteringEngine::new(vec![monitor], compiler, config);

        let tx = TransactionBuilder::new().value(U256::from(150)).build();
        // This item has no logs, but should still be evaluated by the tx monitor
        let item = CorrelatedBlockItem::new(tx, vec![], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].notifier_name, "transaction");
    }

    #[tokio::test]
    async fn test_evaluate_item_no_match_for_tx_monitor() {
        let monitor = create_test_monitor(1, None, None, "bigint(tx.value) > bigint(200)");
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config.clone()));
        let engine = RhaiFilteringEngine::new(vec![monitor], compiler, config);

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
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config.clone()));
        let engine = RhaiFilteringEngine::new(vec![log_monitor, tx_monitor], compiler, config);

        // Create a transaction that will match the transaction-level monitor
        let tx = TransactionBuilder::new().value(U256::from(150)).build();

        // Create a log that will match the log-based monitor
        let log_raw = LogBuilder::new().address(addr).build();
        let log = DecodedLog { name: "Transfer".to_string(), params: vec![], log: log_raw.into() };

        // The item contains both the transaction and the log
        let item = CorrelatedBlockItem::new(tx.into(), vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 2);
        let mut ids: Vec<i64> = matches.iter().map(|m| m.monitor_id).collect();
        ids.sort_unstable();
        assert_eq!(ids, vec![1, 2]);
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
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config.clone()));
        let engine = RhaiFilteringEngine::new(vec![monitor], compiler, config);

        let (tx, log) = create_test_log_and_tx(
            addr,
            "ValueTransfered",
            vec![("value".to_string(), DynSolValue::Uint(U256::from(150), 256))],
        );
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].notifier_name, "ValueTransfered");
        assert_eq!(matches[0].trigger_data["value"], json!(150));
    }

    #[tokio::test]
    async fn test_evaluate_item_no_decoded_logs_still_triggers_tx_monitor() {
        let monitor = create_test_monitor(1, None, None, "true"); // Always matches
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config.clone()));
        let engine = RhaiFilteringEngine::new(vec![monitor], compiler, config);

        let tx = TransactionBuilder::new().build();
        let item = CorrelatedBlockItem::new(tx, vec![], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
    }

    #[tokio::test]
    async fn test_requires_receipt_data_flag_set_correctly() {
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config.clone()));

        // --- Scenario 1: A monitor explicitly uses a receipt field ---
        let monitors_with_receipt_field = vec![
            create_test_monitor(1, None, None, "tx.value > 100"), // No receipt needed
            create_test_monitor(2, None, None, "tx.status == 1"), // Receipt needed!
        ];
        let engine_needs_receipts = RhaiFilteringEngine::new(
            monitors_with_receipt_field,
            Arc::clone(&compiler),
            config.clone(),
        );
        assert_eq!(
            engine_needs_receipts.requires_receipt_data(),
            true,
            "Should require receipts when 'tx.status' is used"
        );

        // --- Scenario 2: No monitors use any receipt fields ---
        let monitors_without_receipt_field = vec![
            create_test_monitor(1, None, None, "tx.value > 100"),
            create_test_monitor(2, None, None, "log.name == \"Transfer\""),
        ];
        let engine_no_receipts = RhaiFilteringEngine::new(
            monitors_without_receipt_field,
            Arc::clone(&compiler),
            config.clone(),
        );
        assert_eq!(
            engine_no_receipts.requires_receipt_data(),
            false,
            "Should not require receipts when no receipt fields are used"
        );

        // --- Scenario 3: A receipt field appears in a comment or string (proves AST
        // analysis works) ---
        let monitors_with_receipt_field_in_comment = vec![
            create_test_monitor(1, None, None, "// This script checks tx.status"),
            create_test_monitor(2, None, None, "tx.value > 100 && log.name == \"tx.gas_used\""),
        ];
        let engine_ast_check = RhaiFilteringEngine::new(
            monitors_with_receipt_field_in_comment,
            Arc::clone(&compiler),
            config.clone(),
        );
        assert_eq!(
            engine_ast_check.requires_receipt_data(),
            false,
            "Should not require receipts when fields are only in comments or strings"
        );

        // --- Scenario 4: A mix of valid and invalid scripts ---
        let monitors_mixed_validity = vec![
            create_test_monitor(1, None, None, "tx.value > 100"), // Valid, no receipt
            create_test_monitor(2, None, None, "tx.gas_used > 50000"), // Valid, needs receipt
            create_test_monitor(3, None, None, "tx.value >"),     // Invalid syntax
        ];
        let engine_mixed = RhaiFilteringEngine::new(
            monitors_mixed_validity,
            Arc::clone(&compiler),
            config.clone(),
        );
        assert_eq!(
            engine_mixed.requires_receipt_data(),
            true,
            "Should require receipts even if other scripts are invalid"
        );
    }
}
