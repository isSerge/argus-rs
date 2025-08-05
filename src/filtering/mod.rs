//! This module defines the `FilteringEngine` and its implementations.
mod rhai_conversions;

use crate::config::RhaiConfig;
use crate::models::correlated_data::CorrelatedBlockItem;
use crate::models::monitor::Monitor;
use crate::models::monitor_match::MonitorMatch;
use async_trait::async_trait;
use dashmap::DashMap;
#[cfg(test)]
use mockall::automock;
use rhai::{AST, Engine, EvalAltResult, Scope};
use rhai_conversions::{
    build_log_map, build_log_params_map, build_transaction_map, build_trigger_data_from_params,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use thiserror::Error;
use tokio::sync::RwLock;
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
    async fn evaluate_item<'a>(
        &self,
        item: &CorrelatedBlockItem<'a>,
    ) -> Result<Vec<MonitorMatch>, Box<dyn std::error::Error + Send + Sync>>;

    /// Updates the set of monitors used by the engine.
    async fn update_monitors(&self, monitors: Vec<Monitor>);

    /// Returns true if any monitor requires transaction receipt data for evaluation.
    /// This allows optimizing data fetching by only including receipts when needed.
    fn requires_receipt_data(&self) -> bool;
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
        // Disable dynamic evaluation
        engine.disable_symbol("eval");

        // Disable module system
        engine.disable_symbol("import");
        engine.disable_symbol("export");

        // Disable I/O operations
        engine.disable_symbol("print");
        engine.disable_symbol("debug");

        // Disable file system access (if available)
        engine.disable_symbol("File");
        engine.disable_symbol("file");

        // Disable network access (if available)
        engine.disable_symbol("http");
        engine.disable_symbol("net");

        // Disable system access (if available)
        engine.disable_symbol("system");
        engine.disable_symbol("process");

        // Disable threading (if available)
        engine.disable_symbol("thread");
        engine.disable_symbol("spawn");
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
    // TODO: add script compilation caching
    #[tracing::instrument(skip(self, item))]
    async fn evaluate_item<'a>(
        &self,
        item: &CorrelatedBlockItem<'a>,
    ) -> Result<Vec<MonitorMatch>, Box<dyn std::error::Error + Send + Sync>> {
        let mut matches = Vec::new();

        // If no monitors are configured, return early
        let monitors_by_address_read_guard = self.monitors_by_address.read().await;
        if monitors_by_address_read_guard.is_empty() {
            return Ok(matches);
        }

        // Build a transaction map for the item with receipt data if available
        let tx_map = build_transaction_map(item.transaction, item.receipt);

        // Iterate over decoded logs in the item
        for log in &item.decoded_logs {
            let log_address_str = log.log.address().to_checksum(None);

            // Efficiently look up monitors for the current log's address
            if let Some(monitors) = monitors_by_address_read_guard.get(&log_address_str) {
                for monitor in monitors.iter() {
                    // Build trigger data from log parameters
                    let params_map = build_log_params_map(&log.params);
                    let log_map = build_log_map(log, params_map);

                    // Build trigger data using the same conversion logic as Rhai for consistency
                    let trigger_data = build_trigger_data_from_params(&log.params);

                    // Create a new scope for the monitor evaluation
                    let mut scope = Scope::new();
                    scope.push("tx", tx_map.clone());
                    scope.push("log", log_map);

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
                                trigger_data,
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
        Monitor {
            id,
            name: format!("Test Monitor {id}"),
            network: "testnet".to_string(),
            address: address.to_string(),
            filter_script: script.to_string(),
        }
    }

    fn create_test_log_and_tx<'a>(
        log_address: Address,
        log_name: &'a str,
        log_params: Vec<(String, DynSolValue)>,
    ) -> (Transaction, DecodedLog<'a>) {
        let tx = TransactionBuilder::new().build();
        let log_raw = LogBuilder::new().address(log_address).build();
        let log = DecodedLog {
            name: log_name.to_string(),
            params: log_params,
            log: Box::leak(Box::new(log_raw)), // Leak to get 'a lifetime
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
        let item = CorrelatedBlockItem::new(&tx, vec![log], None);

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
        let item = CorrelatedBlockItem::new(&tx, vec![log], None);

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
        let item = CorrelatedBlockItem::new(&tx, vec![log], None);

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
        let item = CorrelatedBlockItem::new(&tx, vec![log], None);

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
        let item = CorrelatedBlockItem::new(&tx, vec![log], None);

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
        let item = CorrelatedBlockItem::new(&tx, vec![log], None);

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
        let item = CorrelatedBlockItem::new(&tx, vec![log], None);

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
        let item = CorrelatedBlockItem::new(&tx, vec![], None); // No decoded logs

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

        let item = CorrelatedBlockItem::new(&tx, vec![log1, log2, log3], None);

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
        let item = CorrelatedBlockItem::new(&tx, vec![log], None);

        let result = engine.evaluate_item(&item).await;
        // Runtime errors are handled gracefully - the monitor is skipped but processing continues
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty()); // No matches due to runtime error
    }

    #[tokio::test]
    async fn test_evaluate_item_script_syntax_error() {
        let addr = address!("0000000000000000000000000000000000000001");
        // Script with a syntax error (missing closing parenthesis)
        let monitor =
            create_test_monitor(1, &addr.to_checksum(None), "log.name == \"Transfer\" && (");
        let engine = RhaiFilteringEngine::new(vec![monitor], RhaiConfig::default());

        let (tx, log) = create_test_log_and_tx(addr, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(&tx, vec![log], None);

        let result = engine.evaluate_item(&item).await;
        // The error should be caught during compilation, not evaluation
        assert!(result.is_ok()); // The function returns Ok(matches) even if compilation fails for a monitor
        // The error message is logged internally, and the loop continues. No match is added.
        // We can assert that no matches are found.
        assert!(result.unwrap().is_empty());
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
}
