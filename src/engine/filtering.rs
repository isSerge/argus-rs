//! This module defines the `FilteringEngine`, which is responsible for
//! evaluating incoming blockchain data (transactions and logs) against a set of
//! user-defined Rhai scripts. It implements a script-driven filtering logic,
//! where the behavior of a monitor (transaction-only, global log-aware, or
//! address-specific log-aware) is determined by the static analysis of its
//! `filter_script`.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use futures::future;
#[cfg(test)]
use mockall::automock;
use rhai::{AST, Engine, EvalAltResult, Scope};
use thiserror::Error;
use tokio::{
    sync::{RwLock, mpsc},
    time::timeout,
};

use super::{
    monitor_organizer::{GLOBAL_MONITORS_KEY, MonitorOrganizer, OrganizedMonitors},
    rhai::{
        conversions::{
            build_log_map, build_log_params_map, build_log_params_payload,
            build_transaction_details_payload, build_transaction_map,
        },
        create_engine,
    },
};
use crate::{
    abi::DecodedLog,
    config::RhaiConfig,
    engine::rhai::compiler::{RhaiCompiler, RhaiCompilerError},
    models::{
        correlated_data::CorrelatedBlockItem,
        decoded_block::DecodedBlockData,
        monitor::Monitor,
        monitor_match::{LogDetails, MonitorMatch},
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
    /// rules. The evaluation proceeds in two phases:
    /// 1. Transaction-only monitors are evaluated once per transaction.
    /// 2. Log-aware monitors (both global and address-specific) are evaluated
    ///    for each decoded log within the transaction, with access to both
    ///    transaction and log data.
    /// Returns a vector of `MonitorMatch` if any conditions are met.
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
    /// The organized collection of monitors for filtering.
    monitors: Arc<RwLock<OrganizedMonitors>>,
    /// The Rhai script compiler.
    compiler: Arc<RhaiCompiler>,
    /// The monitor organizer.
    organizer: MonitorOrganizer,
    /// Indicates if any monitor requires transaction receipt data.
    requires_receipts: AtomicBool,
    /// The Rhai script execution configuration.
    config: RhaiConfig,
    /// The Rhai script execution engine.
    engine: Engine,
}

impl RhaiFilteringEngine {
    /// Creates a new `RhaiFilteringEngine` with the given monitors and Rhai
    /// configuration.
    pub fn new(monitors: Vec<Monitor>, compiler: Arc<RhaiCompiler>, config: RhaiConfig) -> Self {
        // Currently use the default Rhai engine creation function, but can consider
        // adding more customizations later
        let engine = create_engine(config.clone());
        let organizer = MonitorOrganizer::new(compiler.clone());

        tracing::debug!(
            monitor_count = monitors.len(),
            "Filtering engine initialized with monitors."
        );

        let (organized_monitors, needs_receipts) = organizer.organize(monitors);

        Self {
            monitors: Arc::new(RwLock::new(organized_monitors)),
            compiler,
            organizer,
            requires_receipts: AtomicBool::new(needs_receipts),
            config,
            engine,
        }
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

    /// Evaluates a single log-aware monitor against a decoded log and its
    /// parent transaction.
    #[tracing::instrument(skip(self, monitor, tx_map, log_map, log_match_payload, decoded_log))]
    async fn evaluate_single_log_monitor(
        &self,
        monitor: Monitor,
        tx_map: rhai::Map,
        log_map: rhai::Map,
        log_match_payload: serde_json::Value,
        tx_match_payload: serde_json::Value,
        decoded_log: &DecodedLog,
    ) -> Result<Vec<MonitorMatch>, RhaiError> {
        let mut scope = Scope::new();
        scope.push("tx", tx_map);
        scope.push("log", log_map);

        let ast = self.compiler.get_ast(&monitor.filter_script)?;

        let block_number = decoded_log.log.block_number().unwrap_or_default();
        let transaction_hash = decoded_log.log.transaction_hash().unwrap_or_default();
        let contract_address = decoded_log.log.address();
        let log_index = decoded_log.log.log_index().unwrap_or_default();

        let mut monitor_matches = Vec::new();
        if let Ok(true) = self.eval_ast_bool_secure(&ast, &mut scope).await {
            for notifier_name in &monitor.notifiers {
                let log_details = LogDetails {
                    contract_address,
                    log_index,
                    name: decoded_log.name.clone(),
                    params: log_match_payload.clone(),
                };

                monitor_matches.push(MonitorMatch::new_log_match(
                    monitor.id,
                    monitor.name.clone(),
                    notifier_name.clone(),
                    block_number,
                    transaction_hash,
                    log_details,
                    tx_match_payload.clone(),
                ));
            }
        }
        Ok(monitor_matches)
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

    #[tracing::instrument(skip(self, item))]
    async fn evaluate_item(
        &self,
        item: &CorrelatedBlockItem,
    ) -> Result<Vec<MonitorMatch>, RhaiError> {
        let mut matches = Vec::new();
        let tx_map = build_transaction_map(&item.transaction, item.receipt.as_ref());

        let monitors_guard = self.monitors.read().await;

        // --- 1. Handle transaction-only monitors ---
        for monitor in &monitors_guard.transaction_only_monitors {
            let mut scope = Scope::new();
            scope.push("tx", tx_map.clone());

            let ast = self.compiler.get_ast(&monitor.filter_script)?;

            if let Ok(true) = self.eval_ast_bool_secure(&ast, &mut scope).await {
                tracing::debug!(
                    monitor_id = monitor.id,
                    tx_hash = %item.transaction.hash(),
                    "Transaction-only monitor condition met."
                );

                // Build transaction data payload for the match
                let tx_match_payload =
                    build_transaction_details_payload(&item.transaction, item.receipt.as_ref());

                for notifier_name in &monitor.notifiers {
                    matches.push(MonitorMatch::new_tx_match(
                        monitor.id,
                        monitor.name.clone(),
                        notifier_name.clone(),
                        item.transaction.block_number().unwrap_or(0),
                        item.transaction.hash(),
                        tx_match_payload.clone(),
                    ));
                }
            }
        }

        // --- 2. Handle all log-aware monitors ---
        if monitors_guard.log_aware_monitors.is_empty() {
            return Ok(matches);
        }

        for log in &item.decoded_logs {
            // Build log data payload for the match
            let log_address_str = log.log.address().to_checksum(None);
            let params_map = build_log_params_map(&log.params);
            let log_map = build_log_map(log, params_map);
            let log_match_payload = build_log_params_payload(&log.params);
            let tx_match_payload =
                build_transaction_details_payload(&item.transaction, item.receipt.as_ref());

            // Evaluate both address-specific and global log-aware monitors.
            let address_monitors = monitors_guard.log_aware_monitors.get(&log_address_str);
            let global_monitors = monitors_guard.log_aware_monitors.get(GLOBAL_MONITORS_KEY);

            let monitors_to_run = address_monitors
                .iter()
                .flat_map(|r| r.iter())
                .chain(global_monitors.iter().flat_map(|r| r.iter()));

            for monitor in monitors_to_run {
                match self
                    .evaluate_single_log_monitor(
                        monitor.clone(),
                        tx_map.clone(),
                        log_map.clone(),
                        log_match_payload.clone(),
                        tx_match_payload.clone(),
                        log,
                    )
                    .await
                {
                    Ok(new_matches) => matches.extend(new_matches),
                    Err(e) => {
                        tracing::error!(
                            monitor_id = monitor.id,
                            error = ?e,
                            "Error evaluating log-aware monitor"
                        );
                    }
                }
            }
        }

        Ok(matches)
    }

    /// Updates the set of monitors used by the engine.
    async fn update_monitors(&self, monitors: Vec<Monitor>) {
        let (new_organized_monitors, needs_receipts) = self.organizer.organize(monitors);

        let mut monitors_guard = self.monitors.write().await;
        *monitors_guard = new_organized_monitors;
        drop(monitors_guard);

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

    use super::*;
    use crate::{
        abi::DecodedLog,
        config::RhaiConfig,
        models::{
            monitor_match::{LogDetails, LogMatchData, MatchData, TransactionMatchData},
            transaction::Transaction,
        },
        test_helpers::{LogBuilder, MonitorBuilder, TransactionBuilder},
    };

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

        let monitor1 = MonitorBuilder::new()
            .id(1)
            .address(addr1_lower)
            .abi("abi.json")
            .filter_script(r#"log.name == "Transfer""#)
            .build();
        let monitor2 = MonitorBuilder::new()
            .id(2)
            .address(addr2_lower)
            .abi("abi.json")
            .filter_script(r#"log.params.value > 100"#)
            .build();
        let monitor3 = MonitorBuilder::new()
            .id(3)
            .address(addr1_lower)
            .abi("abi.json")
            .filter_script(r#"log.name == "Approval""#)
            .build();
        let monitor4 =
            MonitorBuilder::new().id(4).filter_script(r#"tx.value > bigint("0")"#).build();

        // Test `new()`
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config.clone()));
        let engine = RhaiFilteringEngine::new(
            vec![monitor1.clone(), monitor2.clone(), monitor3.clone(), monitor4.clone()],
            compiler,
            config,
        );

        let monitors_guard = engine.monitors.read().await;
        assert_eq!(monitors_guard.log_aware_monitors.len(), 2);
        assert_eq!(monitors_guard.log_aware_monitors.get(addr1_checksum).unwrap().len(), 2);
        assert_eq!(monitors_guard.log_aware_monitors.get(addr2_checksum).unwrap().len(), 1);
        assert_eq!(monitors_guard.transaction_only_monitors.len(), 1);
        assert_eq!(monitors_guard.transaction_only_monitors[0].id, 4);
        drop(monitors_guard);

        // Test `update_monitors()`
        let monitor5 = MonitorBuilder::new()
            .id(5)
            .address(addr2_lower)
            .filter_script(r#"log.name == "Deposit""#)
            .build();
        let monitor6 = MonitorBuilder::new().id(6).filter_script(r#"tx.gas_price > 100"#).build();
        engine.update_monitors(vec![monitor1.clone(), monitor5.clone(), monitor6.clone()]).await;

        let monitors_guard = engine.monitors.read().await;
        assert_eq!(monitors_guard.log_aware_monitors.len(), 2);
        assert_eq!(monitors_guard.log_aware_monitors.get(addr1_checksum).unwrap().len(), 1);
        assert_eq!(monitors_guard.log_aware_monitors.get(addr2_checksum).unwrap().len(), 1);
        assert_eq!(monitors_guard.log_aware_monitors.get(addr2_checksum).unwrap()[0].id, 5);
        assert_eq!(monitors_guard.transaction_only_monitors.len(), 1);
        assert_eq!(monitors_guard.transaction_only_monitors[0].id, 6);
        drop(monitors_guard);
    }

    #[tokio::test]
    async fn test_evaluate_item_log_based_match() {
        let addr = address!("0000000000000000000000000000000000000001");
        let monitor = MonitorBuilder::new()
            .id(1)
            .address(&addr.to_checksum(None))
            .abi("abi.json")
            .filter_script("log.name == \"Transfer\"")
            .notifiers(vec!["notifier1".to_string(), "notifier2".to_string()])
            .build();
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config.clone()));
        let engine = RhaiFilteringEngine::new(vec![monitor], compiler, config);

        let (tx, log) = create_test_log_and_tx(addr, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].notifier_name, "notifier1");
        assert_eq!(matches[1].monitor_id, 1);
        assert_eq!(matches[1].notifier_name, "notifier2");
    }

    #[tokio::test]
    async fn test_evaluate_item_transaction_based_match() {
        let monitor = MonitorBuilder::new()
            .id(1)
            .filter_script("tx.value > bigint(\"100\")")
            .notifiers(vec!["notifier1".to_string()])
            .build();
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config.clone()));
        let engine = RhaiFilteringEngine::new(vec![monitor], compiler, config);

        let tx = TransactionBuilder::new().value(U256::from(150)).build();
        // This item has no logs, but should still be evaluated by the tx monitor
        let item = CorrelatedBlockItem::new(tx, vec![], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].notifier_name, "notifier1");
    }

    #[tokio::test]
    async fn test_evaluate_item_no_match_for_tx_monitor() {
        let monitor = MonitorBuilder::new()
            .id(1)
            .filter_script("tx.value > bigint(\"200\")")
            .notifiers(vec!["notifier1".to_string()])
            .build();
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
        let log_monitor = MonitorBuilder::new()
            .id(1)
            .address(&addr.to_checksum(None))
            .abi("abi.json")
            .filter_script("log.name == \"Transfer\"")
            .notifiers(vec!["log_notifier".to_string()])
            .build();
        let tx_monitor = MonitorBuilder::new()
            .id(2)
            .filter_script("tx.value > bigint(\"100\")")
            .notifiers(vec!["tx_notifier".to_string()])
            .build();
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config.clone()));
        let engine = RhaiFilteringEngine::new(vec![log_monitor, tx_monitor], compiler, config);

        // Create a transaction that will match the transaction-level monitor
        let tx = TransactionBuilder::new().value(U256::from(120)).build();

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
        let monitor = MonitorBuilder::new()
            .id(1)
            .address(&addr.to_checksum(None))
            .abi("abi.json")
            .filter_script("log.name == \"ValueTransfered\" && log.params.value > bigint(\"100\")")
            .notifiers(vec!["notifier1".to_string()])
            .build();
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config.clone()));
        let engine = RhaiFilteringEngine::new(vec![monitor], compiler, config);

        let value = 130;
        let (tx, log) = create_test_log_and_tx(
            addr,
            "ValueTransfered",
            vec![("value".to_string(), DynSolValue::Uint(U256::from(value), 256))],
        );
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].notifier_name, "notifier1");
        assert!(
            matches!(&matches[0].match_data, MatchData::Log(log_match) if log_match.log_details.name == "ValueTransfered")
        );
    }

    #[tokio::test]
    async fn test_evaluate_item_no_decoded_logs_still_triggers_tx_monitor() {
        let monitor = MonitorBuilder::new().id(1).notifiers(vec!["notifier1".to_string()]).build();
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
        let monitor_no_receipt = MonitorBuilder::new()
            .id(1)
            .filter_script("tx.value > bigint(\"100\")") // No receipt needed
            .build();
        let monitor_requires_receipt = MonitorBuilder::new()
            .id(2)
            .filter_script("tx.status == 1") // This requires receipt data
            .build();
        let monitors_with_receipt_field = vec![monitor_no_receipt, monitor_requires_receipt];
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
        let monitor_no_receipt = MonitorBuilder::new()
            .id(1)
            .filter_script("tx.value > bigint(\"100\")") // No receipt needed
            .build();
        let monitor_no_receipt_too = MonitorBuilder::new()
            .id(2)
            .filter_script("log.name == \"Transfer\"") // No receipt needed
            .build();
        let monitors_without_receipt_field = vec![monitor_no_receipt, monitor_no_receipt_too];
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
        let monitor_commented_field =
            MonitorBuilder::new().id(1).filter_script("// This script checks tx.status").build();
        let monitor = MonitorBuilder::new()
            .id(2)
            .filter_script("tx.value > bigint(\"100\") && log.name == \"tx.gas_used\"")
            .build();
        let monitors_with_receipt_field_in_comment = vec![monitor_commented_field, monitor];
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
        let monitor_valid_no_receipt = MonitorBuilder::new()
            .id(1)
            .filter_script("tx.value > bigint(\"100\")") // Valid, no receipt
            .build();
        let monitor_valid_requires_receipt = MonitorBuilder::new()
            .id(2)
            .filter_script("tx.gas_used > bigint(\"50000\")") // Valid, needs receipt
            .build();
        let monitor_invalid = MonitorBuilder::new()
            .id(3)
            .filter_script("tx.value >") // Invalid syntax
            .build();
        let monitors_mixed_validity =
            vec![monitor_valid_no_receipt, monitor_valid_requires_receipt, monitor_invalid];
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

    #[tokio::test]
    async fn test_evaluate_item_with_evm_wrappers() {
        let monitor = MonitorBuilder::new()
            .id(1)
            .filter_script("tx.value > ether(1.5)")
            .notifiers(vec!["notifier1".to_string()])
            .build();
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config.clone()));
        let engine = RhaiFilteringEngine::new(vec![monitor], compiler, config);

        // This transaction's value is 2 ETH, which should trigger the monitor
        let tx_match = TransactionBuilder::new()
            .value(U256::from(2) * U256::from(10).pow(U256::from(18)))
            .build();
        let item_match = CorrelatedBlockItem::new(tx_match.clone(), vec![], None);

        // This transaction's value is 1 ETH, which should NOT trigger the monitor
        let tx_no_match = TransactionBuilder::new()
            .value(U256::from(1) * U256::from(10).pow(U256::from(18)))
            .build();
        let item_no_match = CorrelatedBlockItem::new(tx_no_match.clone(), vec![], None);

        // Test matching case
        let matches = engine.evaluate_item(&item_match).await.unwrap();
        assert_eq!(matches.len(), 1, "Should find one match for value > 1.5 ether");
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].transaction_hash, tx_match.hash());
        assert!(matches!(
            matches[0].match_data,
            MatchData::Transaction(TransactionMatchData { .. })
        ));

        // Test non-matching case
        let no_matches = engine.evaluate_item(&item_no_match).await.unwrap();
        assert!(no_matches.is_empty(), "Should find no matches for value <= 1.5 ether");
    }

    #[tokio::test]
    async fn test_evaluate_item_global_log_monitor_match() {
        // This monitor has no address, so it should run on logs from ANY address.
        let global_monitor = MonitorBuilder::new()
            .id(100)
            .filter_script("log.name == \"GlobalTransfer\"")
            .notifiers(vec!["global_notifier".to_string()])
            .build();

        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config.clone()));
        let engine = RhaiFilteringEngine::new(vec![global_monitor], compiler, config);

        let addr1 = address!("1111111111111111111111111111111111111111");
        let addr2 = address!("2222222222222222222222222222222222222222");

        let (tx, log1) = create_test_log_and_tx(addr1, "GlobalTransfer", vec![]);
        let (_, log2) = create_test_log_and_tx(addr2, "GlobalTransfer", vec![]);
        // This log should be ignored by the monitor
        let (_, log3) = create_test_log_and_tx(addr1, "OtherEvent", vec![]);

        let item = CorrelatedBlockItem::new(tx, vec![log1, log2, log3], None);

        let matches = engine.evaluate_item(&item).await.unwrap();

        // We expect two matches, one for each "GlobalTransfer" log.
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].monitor_id, 100);
        assert_eq!(matches[1].monitor_id, 100);
        assert_eq!(matches[0].block_number, item.transaction.block_number().unwrap_or_default());
        assert!(
            matches!(matches[0].match_data, MatchData::Log(LogMatchData { log_details: LogDetails { contract_address, .. }, .. }) if contract_address == addr1)
        );
        assert_eq!(matches[1].block_number, item.transaction.block_number().unwrap_or_default());
        assert!(
            matches!(matches[1].match_data, MatchData::Log(LogMatchData { log_details: LogDetails { contract_address, .. }, .. }) if contract_address == addr2)
        );
    }
}
