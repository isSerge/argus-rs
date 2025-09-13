//! This module defines the `FilteringEngine`, which is responsible for
//! evaluating incoming blockchain data (transactions and logs) against a set of
//! user-defined Rhai scripts. It implements a script-driven filtering logic,
//! where the behavior of a monitor (transaction-only, global log-aware, or
//! address-specific log-aware) is determined by the static analysis of its
//! `filter_script`.

use std::{collections::HashSet, sync::Arc, time::Duration};

use async_trait::async_trait;
use futures::future;
#[cfg(test)]
use mockall::automock;
use rhai::{AST, Engine, EvalAltResult, Scope};
use thiserror::Error;
use tokio::{sync::mpsc, time::timeout};

use super::rhai::{
    conversions::{
        build_log_map, build_log_params_payload, build_params_map,
        build_transaction_details_payload, build_transaction_map,
    },
    create_engine,
};
use crate::{
    abi::DecodedLog,
    config::RhaiConfig,
    engine::rhai::{
        compiler::{RhaiCompiler, RhaiCompilerError},
        conversions::build_decoded_call_map,
    },
    models::{
        correlated_data::CorrelatedBlockItem,
        decoded_block::DecodedBlockData,
        monitor::Monitor,
        monitor_match::{LogDetails, MonitorMatch},
    },
    monitor::{MonitorCapabilities, MonitorManager},
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
    /// rules. The evaluation proceeds in two phases to ensure correctness and
    /// prevent duplicate notifications:
    /// 1. Log-aware monitors are evaluated against each log in the transaction.
    ///    If a monitor matches any log, it is marked as matched and will not be
    ///    re-evaluated.
    /// 2. Any monitor that did not match in the first pass is then evaluated
    ///    against the transaction-level context.
    /// This ensures that a monitor can match multiple logs, but a monitor that
    /// matches on a log will not also produce a transaction-level match.
    /// Returns a vector of `MonitorMatch` if any conditions are met.
    async fn evaluate_item(
        &self,
        item: &CorrelatedBlockItem,
    ) -> Result<Vec<MonitorMatch>, RhaiError>;

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
    /// The Rhai script compiler.
    compiler: Arc<RhaiCompiler>,

    /// The Rhai script execution configuration.
    config: RhaiConfig,

    /// The Rhai script execution engine.
    engine: Engine,

    /// The monitor manager that holds and manages the monitors.
    monitor_manager: Arc<MonitorManager>,
}

impl RhaiFilteringEngine {
    /// Creates a new `RhaiFilteringEngine` with the given monitors and Rhai
    /// configuration.
    pub fn new(
        compiler: Arc<RhaiCompiler>,
        config: RhaiConfig,
        monitor_manager: Arc<MonitorManager>,
    ) -> Self {
        // Currently use the default Rhai engine creation function, but can consider
        // adding more customizations later
        let engine = create_engine(config.clone());

        Self { compiler, config, engine, monitor_manager }
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

    async fn does_monitor_match(
        &self,
        monitor: &Monitor,
        item: &CorrelatedBlockItem,
        log: Option<&DecodedLog>,
    ) -> Result<bool, RhaiError> {
        let mut scope = Scope::new();

        // Build the context for this specific evaluation
        let tx_map = build_transaction_map(&item.transaction, item.receipt.as_ref());
        scope.push("tx", tx_map);

        let decoded_call_dynamic: rhai::Dynamic = match &item.decoded_call {
            Some(c) => build_decoded_call_map(c).into(),
            None => ().into(),
        };
        scope.push("decoded_call", decoded_call_dynamic);

        if let Some(l) = log {
            let params_map = build_params_map(&l.params);
            let log_map = build_log_map(l, params_map);
            scope.push("log", log_map);
        } else {
            scope.push("log", ()); // Ensure `log` is null when not present
        }

        let ast = self.compiler.get_ast(&monitor.filter_script)?;

        self.eval_ast_bool_secure(&ast, &mut scope).await
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
        let assets = self.monitor_manager.load();
        let mut matched_monitor_ids: HashSet<i64> = HashSet::new();

        // --- First Pass: Evaluate log-aware monitors to get the most specific match
        // --- A single monitor can match multiple logs in the same transaction.
        for cm in assets.monitors.iter() {
            if cm.caps.contains(MonitorCapabilities::LOG) {
                for log in &item.decoded_logs {
                    if self.does_monitor_match(&cm.monitor, item, Some(log)).await? {
                        let log_match_payload = build_log_params_payload(&log.params);

                        for notifier in &cm.monitor.notifiers {
                            let log_details = LogDetails {
                                log_index: log.log.log_index().unwrap_or_default(),
                                address: log.log.address(),
                                name: log.name.clone(),
                                params: log_match_payload.clone(),
                            };

                            matches.push(MonitorMatch::new_log_match(
                                cm.monitor.id,
                                cm.monitor.name.clone(),
                                notifier.clone(),
                                item.transaction.block_number().unwrap_or_default(),
                                item.transaction.hash(),
                                log_details,
                                log_match_payload.clone(),
                            ));
                        }
                        // Mark this monitor as having found at least one log match.
                        matched_monitor_ids.insert(cm.monitor.id);
                    }
                }
            }
        }

        // --- Second Pass: Evaluate any remaining monitors for transaction-only matches
        for cm in assets.monitors.iter() {
            // Skip if this monitor has already produced a log match.
            if matched_monitor_ids.contains(&cm.monitor.id) {
                continue;
            }

            // For any monitor (log-aware or not) that hasn't matched, evaluate it
            // against the transaction context where `log` is null.
            if self.does_monitor_match(&cm.monitor, item, None).await? {
                let tx_match_payload =
                    build_transaction_details_payload(&item.transaction, item.receipt.as_ref());

                for notifier in &cm.monitor.notifiers {
                    matches.push(MonitorMatch::new_tx_match(
                        cm.monitor.id,
                        cm.monitor.name.clone(),
                        notifier.clone(),
                        item.transaction.block_number().unwrap_or_default(),
                        item.transaction.hash(),
                        tx_match_payload.clone(),
                    ));
                }
            }
        }

        Ok(matches)
    }

    /// Returns true if any monitor requires transaction receipt data for
    /// filtering.
    fn requires_receipt_data(&self) -> bool {
        self.monitor_manager.load().requires_receipts
    }
}

#[cfg(test)]
mod tests {
    use alloy::{
        dyn_abi::DynSolValue,
        primitives::{Address, U256, address, b256},
    };
    use tempfile::tempdir;

    use super::*;
    use crate::{
        abi::{AbiService, DecodedLog},
        config::RhaiConfig,
        models::{
            monitor_match::{LogDetails, LogMatchData, MatchData, TransactionMatchData},
            transaction::Transaction,
        },
        test_helpers::{
            LogBuilder, MonitorBuilder, TransactionBuilder, create_test_abi_service,
            simple_abi_json,
        },
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

    fn setup_engine_with_monitors(
        monitors: Vec<Monitor>,
        abi_service: Arc<AbiService>,
    ) -> RhaiFilteringEngine {
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config.clone()));
        let monitor_manager =
            Arc::new(MonitorManager::new(monitors, Arc::clone(&compiler), abi_service));
        RhaiFilteringEngine::new(compiler, config, monitor_manager)
    }

    #[tokio::test]
    async fn test_evaluate_item_log_based_match() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("simple", simple_abi_json())]);

        let addr = address!("0000000000000000000000000000000000000001");
        let monitor = MonitorBuilder::new()
            .id(1)
            .address(&addr.to_checksum(None))
            .abi("simple")
            .filter_script("log.name == \"Transfer\"")
            .notifiers(vec!["notifier1".to_string(), "notifier2".to_string()])
            .build();
        let monitors = vec![monitor];
        let engine = setup_engine_with_monitors(monitors, abi_service);

        let (tx, log) = create_test_log_and_tx(addr, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(tx, vec![log], None, None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].notifier_name, "notifier1");
        assert_eq!(matches[1].monitor_id, 1);
        assert_eq!(matches[1].notifier_name, "notifier2");
    }

    #[tokio::test]
    async fn test_evaluate_item_transaction_based_match() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("simple", simple_abi_json())]);
        let monitor = MonitorBuilder::new()
            .id(1)
            .filter_script("tx.value > bigint(\"100\")")
            .notifiers(vec!["notifier1".to_string()])
            .build();
        let monitors = vec![monitor];
        let engine = setup_engine_with_monitors(monitors, abi_service);

        let tx = TransactionBuilder::new().value(U256::from(150)).build();
        // This item has no logs, but should still be evaluated by the tx monitor
        let item = CorrelatedBlockItem::new(tx, vec![], None, None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].notifier_name, "notifier1");
    }

    #[tokio::test]
    async fn test_evaluate_item_no_match_for_tx_monitor() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("simple", simple_abi_json())]);

        let monitor = MonitorBuilder::new()
            .id(1)
            .filter_script("tx.value > bigint(\"200\")")
            .notifiers(vec!["notifier1".to_string()])
            .build();
        let monitors = vec![monitor];
        let engine = setup_engine_with_monitors(monitors, abi_service);

        let tx = TransactionBuilder::new().value(U256::from(150)).build();
        let item = CorrelatedBlockItem::new(tx, vec![], None, None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert!(matches.is_empty());
    }

    #[tokio::test]
    async fn test_evaluate_item_mixed_monitors_both_match() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("simple", simple_abi_json())]);
        let addr = address!("0000000000000000000000000000000000000001");
        let log_monitor = MonitorBuilder::new()
            .id(1)
            .address(&addr.to_checksum(None))
            .abi("simple")
            .filter_script("log.name == \"Transfer\"")
            .notifiers(vec!["log_notifier".to_string()])
            .build();
        let tx_monitor = MonitorBuilder::new()
            .id(2)
            .filter_script("tx.value > bigint(\"100\")")
            .notifiers(vec!["tx_notifier".to_string()])
            .build();
        let monitors = vec![log_monitor.clone(), tx_monitor.clone()];
        let engine = setup_engine_with_monitors(monitors, abi_service);

        // Create a transaction that will match the transaction-level monitor
        let tx = TransactionBuilder::new().value(U256::from(120)).build();

        // Create a log that will match the log-based monitor
        let log_raw = LogBuilder::new().address(addr).build();
        let log = DecodedLog { name: "Transfer".to_string(), params: vec![], log: log_raw.into() };

        // The item contains both the transaction and the log
        let item = CorrelatedBlockItem::new(tx.into(), vec![log], None, None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 2);
        let mut ids: Vec<i64> = matches.iter().map(|m| m.monitor_id).collect();
        ids.sort_unstable();
        assert_eq!(ids, vec![1, 2]);
    }

    #[tokio::test]
    async fn test_evaluate_item_filter_by_log_param() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("simple", simple_abi_json())]);
        let addr = address!("0000000000000000000000000000000000000001");
        let monitor = MonitorBuilder::new()
            .id(1)
            .address(&addr.to_checksum(None))
            .abi("simple")
            .filter_script("log.name == \"ValueTransfered\" && log.params.value > bigint(\"100\")")
            .notifiers(vec!["notifier1".to_string()])
            .build();
        let monitors = vec![monitor];
        let engine = setup_engine_with_monitors(monitors, abi_service);

        let value = 130;
        let (tx, log) = create_test_log_and_tx(
            addr,
            "ValueTransfered",
            vec![("value".to_string(), DynSolValue::Uint(U256::from(value), 256))],
        );
        let item = CorrelatedBlockItem::new(tx, vec![log], None, None);

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
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("simple", simple_abi_json())]);
        let monitor = MonitorBuilder::new().id(1).notifiers(vec!["notifier1".to_string()]).build();
        let monitors = vec![monitor];
        let engine = setup_engine_with_monitors(monitors, abi_service);
        let tx = TransactionBuilder::new().build();
        let item = CorrelatedBlockItem::new(tx, vec![], None, None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
    }

    #[tokio::test]
    async fn test_evaluate_item_tx_only_monitor_with_decoded_call_match() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("simple", simple_abi_json())]);
        let contract_address = address!("0000000000000000000000000000000000000001");
        abi_service.link_abi(contract_address, "simple").unwrap();

        // This monitor is specific to an address and cares about decoded calldata, but
        // not logs.
        let monitor = MonitorBuilder::new()
            .id(1)
            .address(&contract_address.to_checksum(None))
            .filter_script(
                r#"decoded_call.name == "transfer" && decoded_call.params.amount > bigint(1000)"#,
            )
            .notifiers(vec!["test-notifier".to_string()])
            .build();
        let engine = setup_engine_with_monitors(vec![monitor], abi_service.clone());

        // This transaction matches the monitor's script.
        let matching_input_str = "0xa9059cbb00000000000000000000000011223344556677889900aabbccddeeff1122334400000000000000000000000000000000000000000000000000000000000005dc";
        let tx = TransactionBuilder::new()
            .to(Some(contract_address))
            .input(matching_input_str.parse().unwrap())
            .build();
        let decoded_call = abi_service.decode_function_input(&tx).unwrap();
        let item = CorrelatedBlockItem::new(tx, vec![], Some(decoded_call), None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1, "Should find one match for high-value transfer");
        assert_eq!(matches[0].monitor_id, 1);
    }

    #[tokio::test]
    async fn test_evaluate_item_log_aware_monitor_with_decoded_call_match() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("simple", simple_abi_json())]);
        let contract_address = address!("0000000000000000000000000000000000000001");
        abi_service.link_abi(contract_address, "simple").unwrap();

        // This monitor cares about both logs and decoded calldata.
        let monitor = MonitorBuilder::new()
            .id(1)
            .address(&contract_address.to_checksum(None))
            .filter_script(
                r#"log.name == "Transfer" && decoded_call.name == "transfer" && decoded_call.params.amount > bigint(1000)"#,
            )
            .notifiers(vec!["test-notifier".to_string()])
            .build();
        let engine = setup_engine_with_monitors(vec![monitor], abi_service.clone());

        // This transaction matches the monitor's script.
        let matching_input_str = "0xa9059cbb00000000000000000000000011223344556677889900aabbccddeeff1122334400000000000000000000000000000000000000000000000000000000000005dc";
        let tx = TransactionBuilder::new()
            .to(Some(contract_address))
            .input(matching_input_str.parse().unwrap())
            .build();
        let decoded_call = abi_service.decode_function_input(&tx).unwrap();
        let (_, log) = create_test_log_and_tx(contract_address, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(tx, vec![log], Some(decoded_call), None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1, "Should find one match for high-value transfer with log");
        assert_eq!(matches[0].monitor_id, 1);
    }

    #[tokio::test]
    async fn test_decoded_call_is_null_for_non_matching_selector() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("simple", simple_abi_json())]);
        let monitor = MonitorBuilder::new()
            .id(1)
            .filter_script("decoded_call == ()") // Check for null decoded_call
            .notifiers(vec!["notifier1".to_string()])
            .build();
        let monitors = vec![monitor];
        let engine = setup_engine_with_monitors(monitors, abi_service);

        let tx = TransactionBuilder::new()
            .input(b256!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").into()) // Invalid selector
            .build();
        let item = CorrelatedBlockItem::new(tx, vec![], None, None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1, "Should match when decoded_call is null");
    }

    #[tokio::test]
    async fn test_requires_receipt_data_flag_set_correctly() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("simple", simple_abi_json())]);
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
        let engine = setup_engine_with_monitors(monitors_with_receipt_field, abi_service.clone());
        assert_eq!(
            engine.requires_receipt_data(),
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
        let engine_no_receipts =
            setup_engine_with_monitors(monitors_without_receipt_field, abi_service.clone());
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
        let engine_ast_check =
            setup_engine_with_monitors(monitors_with_receipt_field_in_comment, abi_service.clone());
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
        let engine_mixed = setup_engine_with_monitors(monitors_mixed_validity, abi_service);
        assert_eq!(
            engine_mixed.requires_receipt_data(),
            true,
            "Should require receipts even if other scripts are invalid"
        );
    }

    #[tokio::test]
    async fn test_evaluate_item_with_evm_wrappers() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("simple", simple_abi_json())]);
        let monitor = MonitorBuilder::new()
            .id(1)
            .filter_script("tx.value > ether(1.5)")
            .notifiers(vec!["notifier1".to_string()])
            .build();
        let monitors = vec![monitor];
        let engine = setup_engine_with_monitors(monitors, abi_service);

        // This transaction's value is 2 ETH, which should trigger the monitor
        let tx_match = TransactionBuilder::new()
            .value(U256::from(2) * U256::from(10).pow(U256::from(18)))
            .build();
        let item_match = CorrelatedBlockItem::new(tx_match.clone(), vec![], None, None);

        // This transaction's value is 1 ETH, which should NOT trigger the monitor
        let tx_no_match = TransactionBuilder::new()
            .value(U256::from(1) * U256::from(10).pow(U256::from(18)))
            .build();
        let item_no_match = CorrelatedBlockItem::new(tx_no_match.clone(), vec![], None, None);

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
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("simple", simple_abi_json())]);
        // This monitor has no address, so it should run on logs from ANY address.
        let global_monitor = MonitorBuilder::new()
            .id(100)
            .filter_script("log.name == \"GlobalTransfer\"")
            .notifiers(vec!["global_notifier".to_string()])
            .build();

        let monitors = vec![global_monitor];
        let engine = setup_engine_with_monitors(monitors, abi_service);

        let addr1 = address!("1111111111111111111111111111111111111111");
        let addr2 = address!("2222222222222222222222222222222222222222");

        let (tx, log1) = create_test_log_and_tx(addr1, "GlobalTransfer", vec![]);
        let (_, log2) = create_test_log_and_tx(addr2, "GlobalTransfer", vec![]);
        // This log should be ignored by the monitor
        let (_, log3) = create_test_log_and_tx(addr1, "OtherEvent", vec![]);

        let item = CorrelatedBlockItem::new(tx, vec![log1, log2, log3], None, None);

        let matches = engine.evaluate_item(&item).await.unwrap();

        // We expect two matches, one for each "GlobalTransfer" log.
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].monitor_id, 100);
        assert_eq!(matches[1].monitor_id, 100);
        assert_eq!(matches[0].block_number, item.transaction.block_number().unwrap_or_default());
        assert!(
            matches!(matches[0].match_data, MatchData::Log(LogMatchData { log_details: LogDetails { address, .. }, .. }) if address == addr1)
        );
        assert_eq!(matches[1].block_number, item.transaction.block_number().unwrap_or_default());
        assert!(
            matches!(matches[1].match_data, MatchData::Log(LogMatchData { log_details: LogDetails { address, .. }, .. }) if address == addr2)
        );
    }

    #[tokio::test]
    async fn test_evaluate_item_hybrid_monitor_tx_match_no_logs() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]);

        // This monitor should match on high-value transactions OR on "Transfer" logs.
        let monitor = MonitorBuilder::new()
            .id(1)
            .filter_script(
                r#"
            tx.value > bigint(100) || log?.name == "Transfer"
        "#,
            )
            .notifiers(vec!["test-notifier".to_string()])
            .build();
        let engine = setup_engine_with_monitors(vec![monitor], abi_service.clone());

        // This transaction matches the `tx.value` part of the script and has NO logs.
        let tx = TransactionBuilder::new().value(U256::from(150)).build();
        let item = CorrelatedBlockItem::new(tx, vec![], None, None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1, "Should match on tx.value even with no logs");
        assert_eq!(matches[0].monitor_id, 1);
        assert!(matches!(matches[0].match_data, MatchData::Transaction(_)));
    }

    #[tokio::test]
    async fn test_evaluate_item_hybrid_monitor_log_match_only() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("simple", simple_abi_json())]);
        let addr = address!("0000000000000000000000000000000000000001");

        let monitor = MonitorBuilder::new()
            .id(1)
            .filter_script(
                r#"
            tx.value > bigint(100) || log?.name == "Transfer"
        "#,
            )
            .notifiers(vec!["test-notifier".to_string()])
            .build();
        let engine = setup_engine_with_monitors(vec![monitor], abi_service.clone());

        // This transaction does NOT match the tx.value, but its log does.
        let tx = TransactionBuilder::new().value(U256::from(50)).build();
        let (_, log) = create_test_log_and_tx(addr, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(tx, vec![log], None, None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1, "Should match on log.name");
        assert_eq!(matches[0].monitor_id, 1);
        assert!(matches!(matches[0].match_data, MatchData::Log(_)));
    }

    #[tokio::test]
    async fn test_evaluate_item_hybrid_monitor_prefers_log_match() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("simple", simple_abi_json())]);
        let addr = address!("0000000000000000000000000000000000000001");

        let monitor = MonitorBuilder::new()
            .id(1)
            .filter_script(
                r#"
            tx.value > bigint(100) || log?.name == "Transfer"
        "#,
            )
            .notifiers(vec!["test-notifier".to_string()])
            .build();
        let engine = setup_engine_with_monitors(vec![monitor], abi_service.clone());

        // This transaction matches BOTH the tx.value and the log.name.
        let tx = TransactionBuilder::new().value(U256::from(150)).build();
        let (_, log) = create_test_log_and_tx(addr, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(tx, vec![log], None, None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        // It should only produce ONE match, and it should be the more specific
        // LogMatch.
        assert_eq!(matches.len(), 1, "Should only produce one match");
        assert_eq!(matches[0].monitor_id, 1);
        assert!(matches!(matches[0].match_data, MatchData::Log(_)), "Should prefer LogMatch");
    }

    #[tokio::test]
    async fn test_safe_null_access_on_decoded_call() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]);

        // This script would fail at runtime if the dot operator on a null
        // `decoded_call` was not handled safely.
        let monitor = MonitorBuilder::new()
            .id(1)
            .filter_script(r#"decoded_call?.name == "nonexistent""#)
            .notifiers(vec!["test-notifier".to_string()])
            .build();
        let engine = setup_engine_with_monitors(vec![monitor], abi_service.clone());

        // This item has no decoded_call, so the variable will be `()`.
        let tx = TransactionBuilder::new().build();
        let item = CorrelatedBlockItem::new(tx, vec![], None, None);

        // The script should evaluate to `false` and not error.
        let matches = engine.evaluate_item(&item).await.unwrap();
        assert!(matches.is_empty(), "Should not match and should not error");
    }

    #[tokio::test]
    async fn test_safe_null_access_on_log() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]);

        // This script would fail if `log.name` access on a null `log` errored.
        // This is a transaction-only evaluation context.
        let monitor = MonitorBuilder::new()
            .id(1)
            .filter_script(r#"log?.name == "nonexistent""#)
            .notifiers(vec!["test-notifier".to_string()])
            .build();
        let engine = setup_engine_with_monitors(vec![monitor], abi_service.clone());

        // This item has no logs, so `log` will be `()` during the tx-only pass.
        let tx = TransactionBuilder::new().build();
        let item = CorrelatedBlockItem::new(tx, vec![], None, None);

        // The script should evaluate to `false` and not error.
        let matches = engine.evaluate_item(&item).await.unwrap();
        assert!(matches.is_empty(), "Should not match and should not error");
    }

    #[tokio::test]
    async fn test_safe_null_access_on_decoded_call_with_valid_call() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("simple", simple_abi_json())]);
        let contract_address = address!("0000000000000000000000000000000000000001");
        abi_service.link_abi(contract_address, "simple").unwrap();

        let monitor = MonitorBuilder::new()
            .id(1)
            .address(&contract_address.to_checksum(None))
            .filter_script(r#"decoded_call.name == "transfer""#)
            .notifiers(vec!["test-notifier".to_string()])
            .build();
        let engine = setup_engine_with_monitors(vec![monitor], abi_service.clone());

        // This transaction matches the monitor's script.
        let matching_input_str = "0xa9059cbb00000000000000000000000011223344556677889900aabbccddeeff1122334400000000000000000000000000000000000000000000000000000000000005dc";
        let tx = TransactionBuilder::new()
            .to(Some(contract_address))
            .input(matching_input_str.parse().unwrap())
            .build();
        let decoded_call = abi_service.decode_function_input(&tx).unwrap();
        let item = CorrelatedBlockItem::new(tx, vec![], Some(decoded_call), None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1, "Should find one match for the transfer call");
        assert_eq!(matches[0].monitor_id, 1);
    }
}
