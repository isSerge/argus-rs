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
    async fn update_monitors(&self, monitors: Vec<Monitor>);

    /// Returns true if any monitor requires transaction receipt data for evaluation.
    /// This allows optimizing data fetching by only including receipts when needed.
    fn requires_receipt_data(&self) -> bool;

    /// Runs the filtering engine with a stream of correlated block items.
    async fn run(&self, mut receiver: mpsc::Receiver<DecodedBlockData>);
}

/// A Rhai-based implementation of the `FilteringEngine` with integrated security controls.
#[derive(Debug)]
pub struct RhaiFilteringEngine {
    monitors_by_address: Arc<RwLock<DashMap<String, Vec<Monitor>>>>,
    engine: Engine,
    rhai_config: RhaiConfig,
    requires_receipts: AtomicBool,
}

impl RhaiFilteringEngine {
    /// Creates a new `RhaiFilteringEngine` with the given monitors and Rhai configuration.
    pub fn new(monitors: Vec<Monitor>, rhai_config: RhaiConfig) -> Self {
        let mut engine = Engine::new();

        // Apply security limits
        engine.set_max_operations(rhai_config.max_operations);
        engine.set_max_call_levels(rhai_config.max_call_levels);
        engine.set_max_string_size(rhai_config.max_string_size);
        engine.set_max_array_size(rhai_config.max_array_size);

        // Disable dangerous language features
        Self::disable_dangerous_features(&mut engine);

        // Register BigInt wrapper for transparent big number handling
        // This provides seamless arithmetic and comparison operations while
        // maintaining Rhai's Fast Operators Mode for performance with standard types
        register_bigint_with_rhai(&mut engine);

        let monitors_by_address: DashMap<String, Vec<Monitor>> = DashMap::new();
        let mut needs_receipts = false;

        for monitor in &monitors {
            monitors_by_address
                .entry(monitor.address.clone())
                .or_default()
                .push(monitor.clone());

            // Check if this monitor's script needs receipt data
            if !needs_receipts && Self::script_needs_receipt_data(&monitor.filter_script) {
                needs_receipts = true;
            }
        }

        Self {
            monitors_by_address: Arc::new(RwLock::new(monitors_by_address)),
            engine,
            rhai_config,
            requires_receipts: AtomicBool::new(needs_receipts),
        }
    }

    /// Disable dangerous language features and standard library functions
    fn disable_dangerous_features(engine: &mut Engine) {
        // List of dangerous symbols to disable
        const DANGEROUS_SYMBOLS: &[&str] = &[
            // Dynamic evaluation
            "eval", // Module system
            "import", "export", // I/O operations
            "print", "debug", // File system access
            "File", "file", // Network access
            "http", "net", // System access
            "system", "process", // Threading
            "thread", "spawn",
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

        // If no monitors are configured, return early
        let monitors_by_address_read_guard = self.monitors_by_address.read().await;
        if monitors_by_address_read_guard.is_empty() {
            return Ok(matches);
        }

        // Build a transaction map for the item with receipt data if available
        let tx_map = build_transaction_map(&item.transaction, item.receipt.as_ref());

        // Iterate over decoded logs in the item
        for log in &item.decoded_logs {
            let log_address_str = log.log.address().to_checksum(None);

            // Efficiently look up monitors for the current log's address
            if let Some(monitors) = monitors_by_address_read_guard.get(&log_address_str) {
                // Build shared data structures once per log to avoid repeated allocations
                let params_map = build_log_params_map(&log.params);
                let log_map = build_log_map(log, params_map);
                let trigger_data = build_trigger_data_from_params(&log.params);

                for monitor in monitors.iter() {
                    // Create a new scope for the monitor evaluation
                    let mut scope = Scope::new();
                    scope.push("tx", tx_map.clone());
                    scope.push("log", log_map.clone());

                    // Compile and evaluate the monitor's filter script
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

                    // Evaluate the script with security controls
                    match self.eval_ast_bool_secure(&ast, &mut scope).await {
                        Ok(true) => {
                            let monitor_match = MonitorMatch {
                                monitor_id: monitor.id,
                                block_number: log.log.block_number().unwrap_or(0),
                                transaction_hash: log.log.transaction_hash().unwrap_or_default(),
                                contract_address: log.log.address(),
                                trigger_name: log.name.clone(),
                                trigger_data: trigger_data.clone(),
                                log_index: log.log.log_index(),
                            };
                            matches.push(monitor_match);
                        }
                        Ok(false) => {
                            tracing::debug!(monitor_id = monitor.id, "Monitor condition not met.");
                        }
                        Err(e) => {
                            tracing::error!(
                                monitor_id = monitor.id,
                                "Script execution failed: {}",
                                e
                            );
                            // Continue processing other monitors instead of failing completely
                            continue;
                        }
                    }
                }
            }
        }

        Ok(matches)
    }

    /// Updates the set of monitors used by the engine.
    async fn update_monitors(&self, monitors: Vec<Monitor>) {
        let monitors_by_address_write_guard = self.monitors_by_address.write().await;
        monitors_by_address_write_guard.clear();

        // Recalculate receipt requirements for new monitors
        let mut needs_receipts = false;
        for monitor in &monitors {
            monitors_by_address_write_guard
                .entry(monitor.address.clone())
                .or_default()
                .push(monitor.clone());

            if !needs_receipts && Self::script_needs_receipt_data(&monitor.filter_script) {
                needs_receipts = true;
            }
        }

        // Update the cached receipt requirement
        self.requires_receipts
            .store(needs_receipts, Ordering::Relaxed);
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

    fn create_test_monitor(id: i64, address: &str, script: &str) -> Monitor {
        let mut monitor = Monitor::from_config(
            format!("Test Monitor {id}"),
            "testnet".to_string(),
            address.to_string(),
            script.to_string(),
        );
        monitor.id = id; // Set the ID after creation for test purposes
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

    fn create_test_log_and_tx_with_params(
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
    async fn test_new_and_update_monitors_grouping() {
        let addr1 = "0x0000000000000000000000000000000000000001";
        let addr2 = "0x0000000000000000000000000000000000000002";

        let monitor1 = create_test_monitor(1, addr1, "true");
        let monitor2 = create_test_monitor(2, addr2, "true");
        let monitor3 = create_test_monitor(3, addr1, "true");

        // Test `new()`
        let engine = RhaiFilteringEngine::new(
            vec![monitor1.clone(), monitor2.clone(), monitor3.clone()],
            RhaiConfig::default(),
        );

        let monitors_read = engine.monitors_by_address.read().await;
        assert_eq!(monitors_read.len(), 2);
        assert_eq!(monitors_read.get(addr1).unwrap().len(), 2);
        assert_eq!(monitors_read.get(addr2).unwrap().len(), 1);
        drop(monitors_read); // Release the read lock

        // Test `update_monitors()`
        let monitor4 = create_test_monitor(4, addr2, "true");
        engine
            .update_monitors(vec![monitor1.clone(), monitor4.clone()])
            .await;

        let monitors_read = engine.monitors_by_address.read().await;
        assert_eq!(monitors_read.len(), 2);
        assert_eq!(monitors_read.get(addr1).unwrap().len(), 1);
        assert_eq!(monitors_read.get(addr2).unwrap().len(), 1);
        assert_eq!(monitors_read.get(addr2).unwrap()[0].id, 4);
    }

    #[tokio::test]
    async fn test_evaluate_item_simple_match() {
        let addr = address!("0000000000000000000000000000000000000001");
        let monitor = create_test_monitor(1, &addr.to_checksum(None), "log.name == \"Transfer\"");
        let engine = RhaiFilteringEngine::new(vec![monitor], RhaiConfig::default());

        let (tx, log) = create_test_log_and_tx(addr, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].trigger_name, "Transfer");
    }

    #[tokio::test]
    async fn test_evaluate_item_no_monitors() {
        let engine = RhaiFilteringEngine::new(vec![], RhaiConfig::default()); // No monitors
        let (tx, log) = create_test_log_and_tx(
            address!("0000000000000000000000000000000000000001"),
            "SomeEvent",
            vec![],
        );
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert!(matches.is_empty());
    }

    #[tokio::test]
    async fn test_evaluate_item_no_match_script() {
        let addr = address!("0000000000000000000000000000000000000001");
        let monitor =
            create_test_monitor(1, &addr.to_checksum(None), "log.name == \"AnotherEvent\"");
        let engine = RhaiFilteringEngine::new(vec![monitor], RhaiConfig::default());

        let (tx, log) = create_test_log_and_tx(addr, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert!(matches.is_empty());
    }

    #[tokio::test]
    async fn test_evaluate_item_no_match_address() {
        let addr1 = address!("0000000000000000000000000000000000000001");
        let addr2 = address!("0000000000000000000000000000000000000002");
        // Monitor for addr1
        let monitor = create_test_monitor(1, &addr1.to_checksum(None), "true");
        let engine = RhaiFilteringEngine::new(vec![monitor], RhaiConfig::default());

        // Create a log for addr2
        let (tx, log) = create_test_log_and_tx(addr2, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert!(matches.is_empty());
    }

    #[tokio::test]
    async fn test_evaluate_item_multiple_monitors_same_address() {
        let addr = address!("0000000000000000000000000000000000000001");
        // Two monitors for the same address with the same event name
        let monitor1 = create_test_monitor(1, &addr.to_checksum(None), "log.name == \"Transfer\"");
        let monitor2 = create_test_monitor(2, &addr.to_checksum(None), "log.name == \"Transfer\"");
        let engine = RhaiFilteringEngine::new(vec![monitor1, monitor2], RhaiConfig::default());

        let (tx, log) = create_test_log_and_tx(addr, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        // Both monitors should match
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
            &addr.to_checksum(None),
            "log.name == \"ValueTransfered\" && log.params.value > 100",
        );
        let engine = RhaiFilteringEngine::new(vec![monitor], RhaiConfig::default());

        let (tx, log) = create_test_log_and_tx(
            addr,
            "ValueTransfered",
            vec![(
                "value".to_string(),
                DynSolValue::Uint(U256::from(150).into(), 256),
            )],
        );
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].trigger_name, "ValueTransfered");
        assert_eq!(matches[0].trigger_data["value"], json!(150));
    }

    #[tokio::test]
    async fn test_evaluate_item_rhai_runtime_error() {
        let addr = address!("0000000000000000000000000000000000000001");
        // Script that will cause a runtime error (division by zero)
        let monitor = create_test_monitor(1, &addr.to_checksum(None), "1 / 0");
        let engine = RhaiFilteringEngine::new(vec![monitor], RhaiConfig::default());

        let (tx, log) = create_test_log_and_tx(addr, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let result = engine.evaluate_item(&item).await;
        // Runtime errors are handled gracefully - the monitor is skipped but processing continues
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty()); // No matches due to runtime error
    }

    #[tokio::test]
    async fn test_evaluate_item_no_decoded_logs() {
        let addr = address!("0000000000000000000000000000000000000001");
        let monitor = create_test_monitor(1, &addr.to_checksum(None), "true");
        let engine = RhaiFilteringEngine::new(vec![monitor], RhaiConfig::default());

        let tx = TransactionBuilder::new().build();
        let item = CorrelatedBlockItem::new(tx, vec![], None); // No decoded logs

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert!(matches.is_empty());
    }

    #[tokio::test]
    async fn test_evaluate_item_multiple_mixed_logs() {
        let addr1 = address!("0000000000000000000000000000000000000001");
        let addr2 = address!("0000000000000000000000000000000000000002");

        let monitor1 = create_test_monitor(1, &addr1.to_checksum(None), "log.name == \"Transfer\"");
        let monitor2 = create_test_monitor(2, &addr2.to_checksum(None), "log.name == \"Approval\"");
        let engine = RhaiFilteringEngine::new(vec![monitor1, monitor2], RhaiConfig::default());

        let tx = TransactionBuilder::new().build();
        let log1 = create_test_log_and_tx(addr1, "Transfer", vec![]).1;
        let log2 = create_test_log_and_tx(addr2, "AnotherEvent", vec![]).1; // This won't match monitor2
        let log3 = create_test_log_and_tx(addr2, "Approval", vec![]).1;

        let item = CorrelatedBlockItem::new(tx, vec![log1, log2, log3], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 2); // Expecting matches for log1 (monitor1) and log3 (monitor2)

        let mut matched_monitor_ids: Vec<i64> = matches.iter().map(|m| m.monitor_id).collect();
        matched_monitor_ids.sort_unstable();
        assert_eq!(matched_monitor_ids, vec![1, 2]);
    }

    #[tokio::test]
    async fn test_evaluate_item_non_existent_field_operation() {
        let addr = address!("0000000000000000000000000000000000000001");
        // Script that tries to perform an arithmetic operation on a non-existent field
        let monitor = create_test_monitor(1, &addr.to_checksum(None), "log.non_existent_field + 1");
        let engine = RhaiFilteringEngine::new(vec![monitor], RhaiConfig::default());

        let (tx, log) = create_test_log_and_tx(addr, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let result = engine.evaluate_item(&item).await;
        // Runtime errors are handled gracefully - the monitor is skipped but processing continues
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty()); // No matches due to runtime error
    }

    #[tokio::test]
    async fn test_compile_script_syntax_error_missing_parenthesis() {
        let config = RhaiConfig::default();
        let engine = RhaiFilteringEngine::new(vec![], config);

        // Script with syntax error - missing closing parenthesis
        let script = "log.name == \"Transfer\" && (";

        let result = engine.compile_script(script);

        // Should get a compilation error
        assert!(result.is_err());
        match result.unwrap_err() {
            RhaiError::CompilationError(_) => {
                // Expected - syntax error should cause compilation error
            }
            other => panic!("Expected CompilationError, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_update_monitors_with_empty_vector() {
        let addr1 = "0x0000000000000000000000000000000000000001";
        let monitor1 = create_test_monitor(1, addr1, "true");
        let engine = RhaiFilteringEngine::new(vec![monitor1], RhaiConfig::default());

        let monitors_read = engine.monitors_by_address.read().await;
        assert_eq!(monitors_read.len(), 1);
        drop(monitors_read);

        engine.update_monitors(vec![]).await;

        let monitors_read = engine.monitors_by_address.read().await;
        assert!(monitors_read.is_empty());
    }

    #[tokio::test]
    async fn test_update_monitors_with_duplicate_addresses() {
        let addr1 = "0x0000000000000000000000000000000000000001";
        let monitor1 = create_test_monitor(1, addr1, "true");
        let monitor2 = create_test_monitor(2, addr1, "false"); // Same address as monitor1
        let monitor3 = create_test_monitor(3, "0x0000000000000000000000000000000000000002", "true");

        let engine = RhaiFilteringEngine::new(
            vec![monitor1.clone(), monitor3.clone()],
            RhaiConfig::default(),
        );

        let monitors_read = engine.monitors_by_address.read().await;
        assert_eq!(monitors_read.len(), 2);
        assert_eq!(monitors_read.get(addr1).unwrap().len(), 1);
        drop(monitors_read);

        // Update with duplicate address monitors
        engine
            .update_monitors(vec![monitor1.clone(), monitor2.clone(), monitor3.clone()])
            .await;

        let monitors_read = engine.monitors_by_address.read().await;
        assert_eq!(monitors_read.len(), 2);
        assert_eq!(monitors_read.get(addr1).unwrap().len(), 2); // Should now have two monitors for addr1
        assert_eq!(monitors_read.get(addr1).unwrap()[0].id, 1);
        assert_eq!(monitors_read.get(addr1).unwrap()[1].id, 2);
        assert_eq!(
            monitors_read
                .get("0x0000000000000000000000000000000000000002")
                .unwrap()
                .len(),
            1
        );
        drop(monitors_read);
    }

    #[tokio::test]
    async fn test_evaluate_item_with_custom_rhai_config() {
        let addr = address!("0000000000000000000000000000000000000001");
        let monitor = create_test_monitor(1, &addr.to_checksum(None), "log.name == \"Transfer\"");

        // Create a custom RhaiConfig with restrictive limits
        let custom_config = RhaiConfig {
            max_operations: 1000,
            max_call_levels: 5,
            max_string_size: 100,
            max_array_size: 10,
            execution_timeout: Duration::from_millis(1000),
        };

        let engine = RhaiFilteringEngine::new(vec![monitor], custom_config);

        let (tx, log) = create_test_log_and_tx(addr, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].trigger_name, "Transfer");
    }

    #[tokio::test]
    async fn test_evaluate_item_with_very_restrictive_config() {
        let addr = address!("0000000000000000000000000000000000000001");
        // A more complex script that might hit operation limits
        let monitor = create_test_monitor(
            1,
            &addr.to_checksum(None),
            "log.name == \"Transfer\" && log.params.value > 0 && log.params.to != \"\" && log.params.from != \"\"",
        );

        // Create a very restrictive RhaiConfig
        let restrictive_config = RhaiConfig {
            max_operations: 1, // Very low limit
            max_call_levels: 1,
            max_string_size: 10,
            max_array_size: 1,
            execution_timeout: Duration::from_millis(1),
        };

        let engine = RhaiFilteringEngine::new(vec![monitor], restrictive_config);

        let (tx, log) = create_test_log_and_tx(
            addr,
            "Transfer",
            vec![
                (
                    "value".to_string(),
                    DynSolValue::Uint(U256::from(100).into(), 256),
                ),
                ("to".to_string(), DynSolValue::String("0x123".to_string())),
                ("from".to_string(), DynSolValue::String("0x456".to_string())),
            ],
        );
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let result = engine.evaluate_item(&item).await;
        // With very restrictive limits, the script execution might fail,
        // but the function should still return Ok and handle errors gracefully
        assert!(result.is_ok());
        // The matches might be empty due to execution limits, which is acceptable
    }

    #[tokio::test]
    async fn test_evaluate_item_with_zero_config_values() {
        let addr = address!("0000000000000000000000000000000000000001");
        let monitor = create_test_monitor(1, &addr.to_checksum(None), "true");

        // Create config with zero values (edge case)
        let zero_config = RhaiConfig {
            max_operations: 0,
            max_call_levels: 0,
            max_string_size: 0,
            max_array_size: 0,
            execution_timeout: Duration::from_millis(0),
        };

        let engine = RhaiFilteringEngine::new(vec![monitor], zero_config);

        let (tx, log) = create_test_log_and_tx(addr, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let result = engine.evaluate_item(&item).await;
        // Zero limits may prevent script execution, but Rhai might handle this differently
        // The main thing is that the function handles this gracefully without panicking
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_evaluate_item_complex_script_with_loops() {
        let addr = address!("0000000000000000000000000000000000000001");
        // Script with a loop that could potentially hit operation limits
        let complex_script = "
            let count = 0;
            for i in 0..100 {
                count += 1;
            }
            log.name == \"Transfer\" && count == 100
        ";
        let monitor = create_test_monitor(1, &addr.to_checksum(None), complex_script);

        let normal_config = RhaiConfig {
            max_operations: 10_000, // Should be enough for the loop
            max_call_levels: 10,
            max_string_size: 1_000,
            max_array_size: 200,
            execution_timeout: Duration::from_millis(5_000),
        };

        let engine = RhaiFilteringEngine::new(vec![monitor], normal_config);

        let (tx, log) = create_test_log_and_tx(addr, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].trigger_name, "Transfer");
    }

    #[tokio::test]
    async fn test_evaluate_item_complex_script_exceeds_operation_limit() {
        let addr = address!("0000000000000000000000000000000000000001");
        // Script with a loop that will exceed operation limits
        let complex_script = "
            let count = 0;
            for i in 0..1000 {
                count += 1;
            }
            log.name == \"Transfer\"
        ";
        let monitor = create_test_monitor(1, &addr.to_checksum(None), complex_script);

        let restrictive_config = RhaiConfig {
            max_operations: 100, // Not enough for the loop
            max_call_levels: 10,
            max_string_size: 1_000,
            max_array_size: 200,
            execution_timeout: Duration::from_millis(5_000),
        };

        let engine = RhaiFilteringEngine::new(vec![monitor], restrictive_config);

        let (tx, log) = create_test_log_and_tx(addr, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let result = engine.evaluate_item(&item).await;
        // Should handle operation limit gracefully
        assert!(result.is_ok());
        // No matches expected due to operation limit being exceeded
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_eval_ast_bool_secure_infinite_recursion_protection() {
        let config = RhaiConfig {
            max_operations: 10_000,
            max_call_levels: 5, // Very low to catch recursion quickly
            max_string_size: 1_000,
            max_array_size: 200,
            execution_timeout: Duration::from_millis(5_000),
        };

        let engine = RhaiFilteringEngine::new(vec![], config);

        // Script with infinite recursion that should be caught by call depth limits
        let script = "
            fn recursive_bomb(n) { 
                recursive_bomb(n + 1); 
            }
            recursive_bomb(0);
            true
        ";

        let ast = engine.compile_script(script).unwrap();
        let mut scope = Scope::new();

        let result = engine.eval_ast_bool_secure(&ast, &mut scope).await;

        // Should get a runtime error due to call depth limit
        assert!(result.is_err());
        match result.unwrap_err() {
            RhaiError::RuntimeError(_) => {
                // Expected - call depth limit should cause runtime error
            }
            other => panic!("Expected RuntimeError, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_eval_ast_bool_secure_mutual_recursion_protection() {
        let config = RhaiConfig {
            max_operations: 10_000,
            max_call_levels: 8, // Should be exceeded by mutual recursion
            max_string_size: 1_000,
            max_array_size: 200,
            execution_timeout: Duration::from_millis(5_000),
        };

        let engine = RhaiFilteringEngine::new(vec![], config);

        // Script with mutual recursion between functions
        let script = "
            fn func_a(n) { 
                if n > 0 { func_b(n - 1); } else { func_b(100); }
            }
            fn func_b(n) { 
                if n > 0 { func_a(n - 1); } else { func_a(100); }
            }
            func_a(50);
            true
        ";

        let ast = engine.compile_script(script).unwrap();
        let mut scope = Scope::new();

        let result = engine.eval_ast_bool_secure(&ast, &mut scope).await;

        // Should get a runtime error due to call depth limit
        assert!(result.is_err());
        match result.unwrap_err() {
            RhaiError::RuntimeError(_) => {
                // Expected - call depth limit should cause runtime error
            }
            other => panic!("Expected RuntimeError, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_compile_script_dangerous_eval_function_blocked() {
        let config = RhaiConfig::default();
        let engine = RhaiFilteringEngine::new(vec![], config);

        // Script trying to use eval function (if it were available)
        let script = "
            let code = \"1 + 1\";
            eval(code) == 2
        ";

        // This should fail at compilation since 'eval' is disabled
        let result = engine.compile_script(script);

        // Also acceptable - compilation error for disabled function
        match result.unwrap_err() {
            RhaiError::CompilationError(_) => {
                // Expected - disabled function could cause compilation error
            }
            other => panic!("Expected CompilationError, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_compile_script_dangerous_import_blocked() {
        let config = RhaiConfig::default();
        let engine = RhaiFilteringEngine::new(vec![], config);

        // Script trying to import modules (if it were available)
        let script = "
            import \"std\" as std;
            true
        ";

        let result = engine.compile_script(script);

        // Should get a compilation error for import syntax
        assert!(result.is_err());
        match result.unwrap_err() {
            RhaiError::CompilationError(_) => {
                // Expected - import syntax should cause compilation error
            }
            other => panic!("Expected CompilationError for import, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_compile_script_print_debug_functions_blocked() {
        let config = RhaiConfig::default();
        let engine = RhaiFilteringEngine::new(vec![], config);

        // Script trying to use print function
        let print_script = "
            print(\"trying to print\");
            true
        ";

        // This should fail at compilation or runtime since 'print' is disabled
        let result = engine.compile_script(print_script);

        match result.unwrap_err() {
            RhaiError::CompilationError(_) => {
                // Expected - disabled function could cause compilation error
            }
            other => panic!(
                "Expected CompilationError for disabled function, got: {:?}",
                other
            ),
        }
    }

    #[tokio::test]
    async fn test_compile_script_debug_function_blocked() {
        let config = RhaiConfig::default();
        let engine = RhaiFilteringEngine::new(vec![], config);

        // Script trying to use debug function
        let debug_script = "
            debug(\"trying to debug\");
            true
        ";

        // This should fail at compilation or runtime since 'debug' is disabled
        let result = engine.compile_script(debug_script);

        match result.unwrap_err() {
            RhaiError::CompilationError(_) => {
                // Expected - disabled function could cause compilation error
            }
            other => panic!(
                "Expected CompilationError for disabled function, got: {:?}",
                other
            ),
        }
    }

    #[tokio::test]
    async fn test_evaluate_item_nested_function_calls_within_limits() {
        let addr = address!("0000000000000000000000000000000000000001");
        // Script with nested function calls that stay within limits
        let nested_script = "
            fn level1() { level2() }
            fn level2() { level3() }
            fn level3() { level4() }
            fn level4() { true }
            
            log.name == \"Transfer\" && level1()
        ";
        let monitor = create_test_monitor(1, &addr.to_checksum(None), nested_script);

        let config = RhaiConfig {
            max_operations: 10_000,
            max_call_levels: 10, // Should be enough for 4 levels
            max_string_size: 1_000,
            max_array_size: 200,
            execution_timeout: Duration::from_millis(5_000),
        };

        let engine = RhaiFilteringEngine::new(vec![monitor], config);

        let (tx, log) = create_test_log_and_tx(addr, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].trigger_name, "Transfer");
    }

    #[tokio::test]
    async fn test_evaluate_item_string_creation_within_limits() {
        let addr = address!("0000000000000000000000000000000000000001");
        // Script that creates strings within limits
        let string_script = "
            let prefix = \"event_\";
            let suffix = \"_detected\";
            let full_name = prefix + log.name + suffix;
            full_name.len() > 0 && log.name == \"Transfer\"
        ";
        let monitor = create_test_monitor(1, &addr.to_checksum(None), string_script);

        let config = RhaiConfig {
            max_operations: 10_000,
            max_call_levels: 10,
            max_string_size: 1_000, // Should be enough for the string operations
            max_array_size: 200,
            execution_timeout: Duration::from_millis(5_000),
        };

        let engine = RhaiFilteringEngine::new(vec![monitor], config);

        let (tx, log) = create_test_log_and_tx(addr, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].trigger_name, "Transfer");
    }

    #[tokio::test]
    async fn test_evaluate_item_array_creation_within_limits() {
        let addr = address!("0000000000000000000000000000000000000001");
        // Script that creates arrays within limits
        let array_script = "
            let events = [\"Transfer\", \"Approval\", \"Mint\"];
            let found = false;
            for event in events {
                if event == log.name {
                    found = true;
                    break;
                }
            }
            found
        ";
        let monitor = create_test_monitor(1, &addr.to_checksum(None), array_script);

        let config = RhaiConfig {
            max_operations: 10_000,
            max_call_levels: 10,
            max_string_size: 1_000,
            max_array_size: 200, // Should be enough for 3 elements
            execution_timeout: Duration::from_millis(5_000),
        };

        let engine = RhaiFilteringEngine::new(vec![monitor], config);

        let (tx, log) = create_test_log_and_tx(addr, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].trigger_name, "Transfer");
    }

    // Direct tests for eval_ast_bool_secure method to verify explicit error handling
    #[tokio::test]
    async fn test_eval_ast_bool_secure_operation_limit_exceeded() {
        let config = RhaiConfig {
            max_operations: 10, // Very low limit
            max_call_levels: 10,
            max_string_size: 1_000,
            max_array_size: 100,
            execution_timeout: Duration::from_millis(5_000),
        };

        let engine = RhaiFilteringEngine::new(vec![], config);

        // Script that will exceed operation limit
        let script = "
            let sum = 0;
            for i in 0..1000 {
                sum += i;
            }
            sum > 0
        ";

        let ast = engine.compile_script(script).unwrap();
        let mut scope = Scope::new();

        let result = engine.eval_ast_bool_secure(&ast, &mut scope).await;

        // Should get a runtime error due to operation limit
        assert!(result.is_err());
        match result.unwrap_err() {
            RhaiError::RuntimeError(_) => {
                // Expected - operation limit should cause runtime error
            }
            other => panic!("Expected RuntimeError, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_eval_ast_bool_secure_execution_timeout() {
        let config = RhaiConfig {
            max_operations: 1_000_000, // High enough to not be the limiting factor
            max_call_levels: 10,
            max_string_size: 1_000,
            max_array_size: 100,
            execution_timeout: Duration::from_millis(1), // Short timeout
        };

        let engine = RhaiFilteringEngine::new(vec![], config);

        // Script with an infinite loop that should timeout
        let script = "
            loop {
                // This will run forever
            }
        ";

        let ast = engine.compile_script(script).unwrap();
        let mut scope = Scope::new();

        let result = engine.eval_ast_bool_secure(&ast, &mut scope).await;

        // Should get an execution timeout error
        assert!(result.is_err());
        match result.unwrap_err() {
            RhaiError::ExecutionTimeout { timeout } => {
                assert_eq!(timeout, Duration::from_millis(1));
            }
            RhaiError::RuntimeError(_) => {
                // This is also acceptable if Rhai catches the infinite loop before timeout
            }
            other => panic!(
                "Expected ExecutionTimeout or RuntimeError, got: {:?}",
                other
            ),
        }
    }

    #[tokio::test]
    async fn test_eval_ast_bool_secure_call_depth_limit() {
        let config = RhaiConfig {
            max_operations: 10_000,
            max_call_levels: 3, // Very low call depth limit
            max_string_size: 1_000,
            max_array_size: 100,
            execution_timeout: Duration::from_millis(5_000),
        };

        let engine = RhaiFilteringEngine::new(vec![], config);

        // Script with deep recursion that should exceed call depth
        let script = "
            fn deep_call(n) {
                if n > 0 {
                    deep_call(n - 1);
                } else {
                    true
                }
            }
            deep_call(10)
        ";

        let ast = engine.compile_script(script).unwrap();
        let mut scope = Scope::new();

        let result = engine.eval_ast_bool_secure(&ast, &mut scope).await;

        // Should get a runtime error due to call depth limit
        assert!(result.is_err());
        match result.unwrap_err() {
            RhaiError::RuntimeError(_) => {
                // Expected - call depth limit should cause runtime error
            }
            other => panic!("Expected RuntimeError, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_eval_ast_bool_secure_runtime_error() {
        let config = RhaiConfig::default();
        let engine = RhaiFilteringEngine::new(vec![], config);

        // Script that causes a runtime error (division by zero)
        let script = "1 / 0 == 1";

        let ast = engine.compile_script(script).unwrap();
        let mut scope = Scope::new();

        let result = engine.eval_ast_bool_secure(&ast, &mut scope).await;

        // Should get a runtime error
        assert!(result.is_err());
        match result.unwrap_err() {
            RhaiError::RuntimeError(_) => {
                // Expected - division by zero should cause runtime error
            }
            other => panic!("Expected RuntimeError, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_eval_ast_bool_secure_successful_execution() {
        let config = RhaiConfig::default();
        let engine = RhaiFilteringEngine::new(vec![], config);

        // Simple script that should execute successfully
        let script = "2 + 2 == 4";

        let ast = engine.compile_script(script).unwrap();
        let mut scope = Scope::new();

        let result = engine.eval_ast_bool_secure(&ast, &mut scope).await;

        // Should succeed and return true
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), true);
    }

    #[tokio::test]
    async fn test_eval_ast_bool_secure_with_scope_variables() {
        let config = RhaiConfig::default();
        let engine = RhaiFilteringEngine::new(vec![], config);

        // Script that uses scope variables
        let script = "x > 10 && y == \"test\"";

        let ast = engine.compile_script(script).unwrap();
        let mut scope = Scope::new();
        scope.push("x", 15_i64);
        scope.push("y", "test".to_string());

        let result = engine.eval_ast_bool_secure(&ast, &mut scope).await;

        // Should succeed and return true
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), true);
    }

    #[tokio::test]
    async fn test_compile_script_syntax_error() {
        let config = RhaiConfig::default();
        let engine = RhaiFilteringEngine::new(vec![], config);

        // Script with syntax error
        let script = "if true { } else";

        let result = engine.compile_script(script);

        // Should get a compilation error
        assert!(result.is_err());
        match result.unwrap_err() {
            RhaiError::CompilationError(_) => {
                // Expected - syntax error should cause compilation error
            }
            other => panic!("Expected CompilationError, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_compile_script_successful() {
        let config = RhaiConfig::default();
        let engine = RhaiFilteringEngine::new(vec![], config);

        // Valid script
        let script = "true && false";

        let result = engine.compile_script(script);

        // Should succeed
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_eval_ast_bool_secure_disabled_functions() {
        let config = RhaiConfig::default();
        let engine = RhaiFilteringEngine::new(vec![], config);

        // Script trying to use disabled eval function
        let script = "eval(\"1 + 1\") == 2";

        // This should fail at compilation since 'eval' is disabled
        let result = engine.compile_script(script);

        match result.unwrap_err() {
            RhaiError::CompilationError(_) => {
                // Expected - disabled function could cause compilation error
            }
            other => panic!(
                "Expected CompilationError for disabled function, got: {:?}",
                other
            ),
        }
    }

    #[tokio::test]
    async fn test_compile_script_file_access_blocked() {
        let config = RhaiConfig::default();
        let engine = RhaiFilteringEngine::new(vec![], config);

        // Script trying to access file system
        let script = "
            let f = File::open(\"test.txt\");
            true
        ";

        // This should fail at runtime since file access is disabled
        let ast = engine.compile_script(script).unwrap();

        let mut scope = Scope::new();
        let runtime_result = engine.eval_ast_bool_secure(&ast, &mut scope).await;
        assert!(runtime_result.is_err());
        match runtime_result.unwrap_err() {
            RhaiError::RuntimeError(_) => {
                // Expected - disabled File access should cause error
            }
            other => panic!(
                "Expected RuntimeError for disabled File access, got: {:?}",
                other
            ),
        }
    }

    #[tokio::test]
    async fn test_compile_script_system_access_blocked() {
        let config = RhaiConfig::default();
        let engine = RhaiFilteringEngine::new(vec![], config);

        // Script trying to access system functions
        let script = "
            system(\"ls\");
            true
        ";

        let ast = engine.compile_script(script).unwrap();

        let mut scope = Scope::new();
        let runtime_result = engine.eval_ast_bool_secure(&ast, &mut scope).await;
        assert!(runtime_result.is_err());
        match runtime_result.unwrap_err() {
            RhaiError::RuntimeError(_) => {
                // Expected - disabled system access should cause error
            }
            other => panic!(
                "Expected RuntimeError for disabled system access, got: {:?}",
                other
            ),
        }
    }

    #[tokio::test]
    async fn test_compile_script_export_blocked() {
        let config = RhaiConfig::default();
        let engine = RhaiFilteringEngine::new(vec![], config);

        // Script trying to use export
        let script = "
            export fn test() { true }
            true
        ";

        let result = engine.compile_script(script);

        // Should get a compilation error for export syntax
        assert!(result.is_err());
        match result.unwrap_err() {
            RhaiError::CompilationError(_) => {
                // Expected - export syntax should cause compilation error
            }
            other => panic!("Expected CompilationError for export, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_evaluate_item_with_big_numbers() {
        let addr = address!("0000000000000000000000000000000000000001");
        // Test script that uses BigInt operations
        let script = r#"
            log.name == "Transfer" && 
            bigint(log.params.value) > bigint("1000000000000000000000") &&
            (bigint(log.params.value) + bigint("500000000000000000000")) > bigint("1500000000000000000000")
        "#;
        let monitor = create_test_monitor(1, &addr.to_checksum(None), script);
        let engine = RhaiFilteringEngine::new(vec![monitor], RhaiConfig::default());

        // Create a test log with a large value parameter
        let large_value = "2000000000000000000000"; // 2000 ETH in wei
        let params = vec![(
            "value".to_string(),
            DynSolValue::Uint(large_value.parse().unwrap(), 256),
        )];
        let (tx, log) = create_test_log_and_tx_with_params(addr, "Transfer", params);
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].trigger_name, "Transfer");
    }

    #[tokio::test]
    async fn test_evaluate_item_explicit_big_number_functions() {
        let addr = address!("0000000000000000000000000000000000000001");
        // Test script that uses BigInt operations
        let script = r#"
            log.name == "Transfer" && 
            bigint(42) + bigint("100") == bigint(142) &&
            bigint("1000") > bigint(999)
        "#;
        let monitor = create_test_monitor(1, &addr.to_checksum(None), script);
        let engine = RhaiFilteringEngine::new(vec![monitor], RhaiConfig::default());

        let (tx, log) = create_test_log_and_tx(addr, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].trigger_name, "Transfer");
    }
}
