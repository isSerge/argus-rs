//! This module defines the `FilteringEngine`, which is responsible for
//! evaluating incoming blockchain data (transactions and logs) against a set of
//! user-defined Rhai scripts. It implements a script-driven filtering logic,
//! where the behavior of a monitor (transaction-only, global log-aware, or
//! address-specific log-aware) is determined by the static analysis of its
//! `filter_script`.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use futures::future;
#[cfg(test)]
use mockall::automock;
use rhai::{AST, Engine, EvalAltResult, Map, Scope};
use thiserror::Error;
use tokio::{sync::mpsc, time::timeout};

use super::rhai::{
    conversions::{
        build_log_params_payload, build_transaction_details_payload, build_transaction_map,
    },
    create_engine,
    proxies::{CallProxy, LogProxy},
};
use crate::{
    abi::{AbiService, DecodedCall, DecodedLog},
    config::RhaiConfig,
    engine::rhai::{RhaiCompiler, RhaiCompilerError},
    models::{
        correlated_data::CorrelatedBlockItem,
        decoded_block::CorrelatedBlockData,
        log::Log,
        monitor::Monitor,
        monitor_match::{LogDetails, MonitorMatch},
    },
    monitor::{ClassifiedMonitor, MonitorCapabilities, MonitorManager},
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
    /// rules.
    async fn evaluate_item(
        &self,
        item: &CorrelatedBlockItem,
    ) -> Result<Vec<MonitorMatch>, RhaiError>;

    /// Returns true if any monitor requires transaction receipt data for
    /// evaluation.
    fn requires_receipt_data(&self) -> bool;

    /// Runs the filtering engine with a stream of correlated block items and
    /// sends matches to the notification queue.
    async fn run(
        &self,
        mut receiver: mpsc::Receiver<CorrelatedBlockData>,
        notifications_tx: mpsc::Sender<MonitorMatch>,
    );
}

/// A Rhai-based implementation of the `FilteringEngine` with integrated
/// security controls.
#[derive(Debug)]
pub struct RhaiFilteringEngine {
    abi_service: Arc<AbiService>,
    compiler: Arc<RhaiCompiler>,
    config: RhaiConfig,
    engine: Engine,
    monitor_manager: Arc<MonitorManager>,
}

/// Holds the transient state for evaluating a single `CorrelatedBlockItem`.
/// This includes caches for decoded data to avoid redundant work.
struct EvaluationContext<'a> {
    item: &'a CorrelatedBlockItem,
    tx_map: Map,
    matches: Vec<MonitorMatch>,
    matched_monitor_ids: HashSet<i64>,
    decoded_logs_cache: HashMap<Log, Arc<DecodedLog>>,
    decoded_call_cache: Option<Option<Arc<DecodedCall>>>,
}

impl<'a> EvaluationContext<'a> {
    fn new(item: &'a CorrelatedBlockItem) -> Self {
        Self {
            item,
            tx_map: build_transaction_map(&item.transaction, item.receipt.as_ref()),
            matches: Vec::new(),
            matched_monitor_ids: HashSet::new(),
            decoded_logs_cache: HashMap::new(),
            decoded_call_cache: None,
        }
    }

    /// Checks if a monitor has already produced a match for this item.
    fn has_matched(&self, monitor_id: i64) -> bool {
        self.matched_monitor_ids.contains(&monitor_id)
    }

    /// Marks a monitor as having produced a match.
    fn mark_as_matched(&mut self, monitor_id: i64) {
        self.matched_monitor_ids.insert(monitor_id);
    }
}

impl RhaiFilteringEngine {
    /// Creates a new `RhaiFilteringEngine` instance.
    pub fn new(
        abi_service: Arc<AbiService>,
        compiler: Arc<RhaiCompiler>,
        config: RhaiConfig,
        monitor_manager: Arc<MonitorManager>,
    ) -> Self {
        let engine = create_engine(config.clone());
        Self { abi_service, compiler, config, engine, monitor_manager }
    }

    /// First evaluation pass: checks log-aware monitors against each log.
    async fn evaluate_log_aware_monitors(
        &self,
        context: &mut EvaluationContext<'_>,
        monitors: &[ClassifiedMonitor],
    ) -> Result<(), RhaiError> {
        for cm in monitors {
            if cm.caps.contains(MonitorCapabilities::LOG) {
                for log in &context.item.logs {
                    let (is_match, decoded_log) =
                        self.does_monitor_match(context, cm, Some(log)).await?;

                    if is_match && let Some(decoded) = decoded_log {
                        self.create_log_matches(context, &cm.monitor, &decoded);
                        context.mark_as_matched(cm.monitor.id);
                    }
                }
            }
        }
        Ok(())
    }

    /// Second evaluation pass: checks remaining monitors in a transaction-only
    /// context.
    async fn evaluate_tx_aware_monitors(
        &self,
        context: &mut EvaluationContext<'_>,
        monitors: &[ClassifiedMonitor],
    ) -> Result<(), RhaiError> {
        for cm in monitors {
            if context.has_matched(cm.monitor.id) {
                continue;
            }

            let is_log_only = cm.caps.contains(MonitorCapabilities::LOG)
                && !cm.caps.contains(MonitorCapabilities::TX)
                && !cm.caps.contains(MonitorCapabilities::CALL);

            if is_log_only {
                continue;
            }

            let (is_match, _) = self.does_monitor_match(context, cm, None).await?;
            if is_match {
                self.create_tx_matches(context, &cm.monitor);
            }
        }
        Ok(())
    }

    /// Core script evaluation logic with lazy decoding and proxy objects.
    async fn does_monitor_match<'a>(
        &self,
        context: &mut EvaluationContext<'a>,
        cm: &ClassifiedMonitor,
        log: Option<&'a Log>,
    ) -> Result<(bool, Option<Arc<DecodedLog>>), RhaiError> {
        tracing::debug!("Evaluating monitor ID {}: {}", cm.monitor.id, cm.monitor.name);
        let mut scope = Scope::new();
        scope.push_constant("tx", context.tx_map.clone());

        // --- Handle Log Data (Lazy Decoding) ---
        let mut decoded_log_result = None;
        if let Some(raw_log) = log {
            if let Some(cached) = context.decoded_logs_cache.get(raw_log) {
                tracing::debug!(
                    "Using cached decoded log for monitor ID {}: {}",
                    cm.monitor.id,
                    cm.monitor.name
                );
                decoded_log_result = Some(cached.clone());
            } else {
                match self.abi_service.decode_log(raw_log) {
                    Ok(decoded) => {
                        tracing::debug!(
                            monitor_id = cm.monitor.id,
                            monitor_name = %cm.monitor.name,
                            log_address = %raw_log.address(),
                            log_topics = ?raw_log.topics(),
                            decoded_log_name = %decoded.name,
                            decoded_params = ?decoded.params,
                            "Successfully decoded log"
                        );
                        let arc_decoded = Arc::new(decoded);
                        context.decoded_logs_cache.insert(raw_log.clone(), arc_decoded.clone());
                        decoded_log_result = Some(arc_decoded);
                    }
                    Err(e) => {
                        tracing::debug!(
                            "Log decoding failed for monitor ID {}: {} - Error: {}",
                            cm.monitor.id,
                            cm.monitor.name,
                            e
                        );
                        // Log decoding failed, continue
                    }
                }
            }
        }

        // --- Handle Calldata (Lazy Decoding) ---
        let mut decoded_call_result = None;
        if cm.caps.contains(MonitorCapabilities::CALL) {
            if context.decoded_call_cache.is_none() {
                context.decoded_call_cache = Some(
                    self.abi_service
                        .decode_function_input(&context.item.transaction)
                        .ok()
                        .map(Arc::new),
                );
            }
            decoded_call_result = context.decoded_call_cache.as_ref().unwrap().clone();
        }

        // --- Push Proxies to Scope ---
        // The proxies handle the `None` case internally, preventing script errors.
        scope.push("log", LogProxy(decoded_log_result.clone()));
        scope.push("decoded_call", CallProxy(decoded_call_result));

        let ast = self.compiler.get_ast(&cm.monitor.filter_script)?;
        tracing::debug!(
            monitor_id = cm.monitor.id,
            monitor_name = %cm.monitor.name,
            script = ?cm.monitor.filter_script,
            "Executing filter script"
        );
        let is_match = self.eval_ast_bool_secure(&ast, &mut scope).await?;
        tracing::debug!(
            monitor_id = cm.monitor.id,
            monitor_name = %cm.monitor.name,
            is_match = is_match,
            "Filter script execution completed"
        );
        Ok((is_match, decoded_log_result))
    }

    /// Creates and stores log-based monitor matches in the context.
    fn create_log_matches(
        &self,
        context: &mut EvaluationContext<'_>,
        monitor: &Monitor,
        decoded_log: &DecodedLog,
    ) {
        let log_match_payload = build_log_params_payload(&decoded_log.params);
        let tx_details = build_transaction_details_payload(
            &context.item.transaction,
            context.item.receipt.as_ref(),
        );
        for action in &monitor.actions {
            let log_details = LogDetails {
                log_index: decoded_log.log.log_index().unwrap_or_default(),
                address: decoded_log.log.address(),
                name: decoded_log.name.clone(),
                params: log_match_payload.clone(),
            };
            context.matches.push(MonitorMatch::new_log_match(
                monitor.id,
                monitor.name.clone(),
                action.clone(),
                context.item.transaction.block_number().unwrap_or_default(),
                context.item.transaction.hash(),
                log_details,
                tx_details.clone(),
            ));
        }
    }

    /// Creates and stores transaction-based monitor matches in the context.
    fn create_tx_matches(&self, context: &mut EvaluationContext<'_>, monitor: &Monitor) {
        let tx_match_payload = build_transaction_details_payload(
            &context.item.transaction,
            context.item.receipt.as_ref(),
        );
        for action in &monitor.actions {
            context.matches.push(MonitorMatch::new_tx_match(
                monitor.id,
                monitor.name.clone(),
                action.clone(),
                context.item.transaction.block_number().unwrap_or_default(),
                context.item.transaction.hash(),
                tx_match_payload.clone(),
            ));
        }
    }

    /// Executes a pre-compiled AST with security controls including a timeout.
    async fn eval_ast_bool_secure(
        &self,
        ast: &AST,
        scope: &mut Scope<'_>,
    ) -> Result<bool, RhaiError> {
        let start_time = std::time::Instant::now();
        let execution = async { self.engine.eval_ast_with_scope::<bool>(scope, ast) };
        match timeout(self.config.execution_timeout, execution).await {
            Ok(result) => {
                let duration = start_time.elapsed();
                tracing::debug!(
                    execution_time_ms = duration.as_millis(),
                    timeout_ms = self.config.execution_timeout.as_millis(),
                    "Script execution completed"
                );
                result.map_err(|e| {
                    tracing::warn!(error = %e, "Script runtime error");
                    RhaiError::RuntimeError(e)
                })
            },
            Err(_) => {
                let duration = start_time.elapsed();
                tracing::error!(
                    execution_time_ms = duration.as_millis(),
                    timeout_ms = self.config.execution_timeout.as_millis(),
                    "Script execution timeout"
                );
                Err(RhaiError::ExecutionTimeout { timeout: self.config.execution_timeout })
            },
        }
    }
}

#[async_trait]
impl FilteringEngine for RhaiFilteringEngine {
    async fn run(
        &self,
        mut receiver: mpsc::Receiver<CorrelatedBlockData>,
        notifications_tx: mpsc::Sender<MonitorMatch>,
    ) {
        while let Some(correlated_block) = receiver.recv().await {
            let futures = correlated_block.items.iter().map(|item| self.evaluate_item(item));
            let results = future::join_all(futures).await;

            for result in results {
                match result {
                    Ok(matches) if !matches.is_empty() =>
                        for monitor_match in matches {
                            if let Err(e) = notifications_tx.send(monitor_match).await {
                                tracing::error!("Failed to send notification match: {}", e);
                            }
                        },
                    Ok(_) => {} // No matches, do nothing.
                    Err(e) => {
                        match &e {
                            RhaiError::ExecutionTimeout { timeout } => {
                                tracing::error!(
                                    timeout_ms = timeout.as_millis(),
                                    "Script execution timeout while evaluating item"
                                );
                            },
                            RhaiError::RuntimeError(runtime_err) => {
                                tracing::error!(
                                    error = %runtime_err,
                                    "Script runtime error while evaluating item"
                                );
                            },
                            RhaiError::CompilationError(compile_err) => {
                                tracing::error!(
                                    error = %compile_err,
                                    "Script compilation error while evaluating item"
                                );
                            },
                        }
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
        let assets = self.monitor_manager.load();
        let mut context = EvaluationContext::new(item);
        
        tracing::debug!(
            tx_hash = %item.transaction.hash(),
            log_count = item.logs.len(),
            monitor_count = assets.monitors.len(),
            "Starting evaluation of correlated block item"
        );

        // --- First Pass: Evaluate log-aware monitors ---
        self.evaluate_log_aware_monitors(&mut context, &assets.monitors).await?;

        // --- Second Pass: Evaluate transaction-aware monitors ---
        self.evaluate_tx_aware_monitors(&mut context, &assets.monitors).await?;

        tracing::debug!(
            tx_hash = %item.transaction.hash(),
            match_count = context.matches.len(),
            "Completed evaluation of correlated block item"
        );

        Ok(context.matches)
    }

    fn requires_receipt_data(&self) -> bool {
        self.monitor_manager.load().requires_receipts
    }
}

#[cfg(test)]
mod tests {
    use alloy::{
        primitives::{Address, B256, Bytes, U256, address, b256},
        sol_types::SolValue,
    };
    use tempfile::tempdir;

    use super::*;
    use crate::{
        abi::AbiService,
        config::RhaiConfig,
        models::{
            monitor_match::{LogDetails, LogMatchData, MatchData, TransactionMatchData},
            transaction::Transaction,
        },
        test_helpers::{
            LogBuilder, MonitorBuilder, TransactionBuilder, create_test_abi_service, erc20_abi_json,
        },
    };

    const TRANSFER_EVENT_TOPIC: B256 =
        b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef");
    const FROM_TOPIC: B256 =
        b256!("000000000000000000000000a0b86a33e6ba3e10e4b86c8c5a3c6b2e6a2e8f1e");
    const TO_TOPIC: B256 =
        b256!("000000000000000000000000b1c97a44f7ca4e21f5b97d8d6a4d7c3f7b3f9e2f");
    const CONTRACT_ADDRESS: Address = address!("0000000000000000000000000000000000000001");
    const TX_INPUT_DATA: &str = "0xa9059cbb00000000000000000000000011223344556677889900aabbccddeeff1122334400000000000000000000000000000000000000000000000000000000000005dc";

    fn create_test_log_and_tx_with_topics(
        log_address: Address,
        topics: Vec<B256>,
        data: Bytes,
    ) -> (Transaction, Log) {
        let tx = TransactionBuilder::new().build();
        let log = LogBuilder::new().address(log_address).topics(topics).data(data).build();
        (tx, log)
    }

    fn setup_engine_with_monitors(
        monitors: Vec<Monitor>,
        abi_service: Arc<AbiService>,
    ) -> RhaiFilteringEngine {
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config.clone()));
        let monitor_manager =
            Arc::new(MonitorManager::new(monitors, Arc::clone(&compiler), abi_service.clone()));
        RhaiFilteringEngine::new(abi_service, compiler, config, monitor_manager)
    }

    #[tokio::test]
    async fn test_evaluate_item_log_based_match() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json())]);

        abi_service.link_abi(CONTRACT_ADDRESS, "erc20").unwrap();

        let monitor = MonitorBuilder::new()
            .id(1)
            .address(&CONTRACT_ADDRESS.to_checksum(None))
            .abi("erc20")
            .filter_script("log.name == \"Transfer\" ")
            .actions(vec!["action1".to_string(), "action2".to_string()])
            .build();
        let monitors = vec![monitor];
        let engine = setup_engine_with_monitors(monitors, abi_service.clone());

        let amount_data = U256::from(1000).abi_encode().into();
        let (tx, log) = create_test_log_and_tx_with_topics(
            CONTRACT_ADDRESS,
            vec![TRANSFER_EVENT_TOPIC, FROM_TOPIC, TO_TOPIC],
            amount_data,
        );
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].action_name, "action1");
        assert_eq!(matches[1].monitor_id, 1);
        assert_eq!(matches[1].action_name, "action2");
    }

    #[tokio::test]
    async fn test_evaluate_item_transaction_based_match() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json())]);
        let monitor = MonitorBuilder::new()
            .id(1)
            .filter_script("tx.value > bigint(\"100\")")
            .actions(vec!["action1".to_string()])
            .build();
        let monitors = vec![monitor];
        let engine = setup_engine_with_monitors(monitors, abi_service);

        let tx = TransactionBuilder::new().value(U256::from(150)).build();
        // This item has no logs, but should still be evaluated by the tx monitor
        let item = CorrelatedBlockItem::new(tx, vec![], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].action_name, "action1");
    }

    #[tokio::test]
    async fn test_evaluate_item_no_match_for_tx_monitor() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json())]);

        let monitor = MonitorBuilder::new()
            .id(1)
            .filter_script("tx.value > bigint(\"200\")")
            .actions(vec!["action1".to_string()])
            .build();
        let monitors = vec![monitor];
        let engine = setup_engine_with_monitors(monitors, abi_service);

        let tx = TransactionBuilder::new().value(U256::from(150)).build();
        let item = CorrelatedBlockItem::new(tx, vec![], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert!(matches.is_empty());
    }

    #[tokio::test]
    async fn test_evaluate_item_mixed_monitors_both_match() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json())]);
        abi_service.link_abi(CONTRACT_ADDRESS, "erc20").unwrap();

        let log_monitor = MonitorBuilder::new()
            .id(1)
            .address(&CONTRACT_ADDRESS.to_checksum(None))
            .abi("erc20")
            .filter_script("log.name == \"Transfer\" ")
            .actions(vec!["log_action".to_string()])
            .build();
        let tx_monitor = MonitorBuilder::new()
            .id(2)
            .filter_script("tx.value > bigint(\"100\")")
            .actions(vec!["tx_action".to_string()])
            .build();
        let monitors = vec![log_monitor.clone(), tx_monitor.clone()];
        let engine = setup_engine_with_monitors(monitors, abi_service);

        // Create a transaction that will match the transaction-level monitor
        let tx = TransactionBuilder::new().value(U256::from(120)).build();

        // Create a log that will match the log-based monitor
        let amount_data = U256::from(1000).abi_encode().into();
        let log = LogBuilder::new()
            .address(CONTRACT_ADDRESS)
            .topics(vec![TRANSFER_EVENT_TOPIC, FROM_TOPIC, TO_TOPIC])
            .data(amount_data)
            .build();

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
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json())]);
        abi_service.link_abi(CONTRACT_ADDRESS, "erc20").unwrap();

        let monitor = MonitorBuilder::new()
            .id(1)
            .address(&CONTRACT_ADDRESS.to_checksum(None))
            .abi("erc20")
            .filter_script("log.name == \"Transfer\" && log.params.value > bigint(\"100\")")
            .actions(vec!["action1".to_string()])
            .build();
        let monitors = vec![monitor];
        let engine = setup_engine_with_monitors(monitors, abi_service);

        // keccak256("Transfer(address,address,uint256)")
        let amount_data = U256::from(150).abi_encode().into();
        let (tx, log) = create_test_log_and_tx_with_topics(
            CONTRACT_ADDRESS,
            vec![TRANSFER_EVENT_TOPIC, FROM_TOPIC, TO_TOPIC],
            amount_data,
        );

        let item = CorrelatedBlockItem::new(tx.into(), vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].action_name, "action1");
        assert!(
            matches!(&matches[0].match_data, MatchData::Log(log_match) if log_match.log_details.name == "Transfer")
        );
    }

    #[tokio::test]
    async fn test_evaluate_item_no_decoded_logs_still_triggers_tx_monitor() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json())]);
        let monitor = MonitorBuilder::new().id(1).actions(vec!["action1".to_string()]).build();
        let monitors = vec![monitor];
        let engine = setup_engine_with_monitors(monitors, abi_service);
        let tx = TransactionBuilder::new().build();
        let item = CorrelatedBlockItem::new(tx, vec![], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
    }

    #[tokio::test]
    async fn test_evaluate_item_tx_only_monitor_with_decoded_call_match() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json())]);
        abi_service.link_abi(CONTRACT_ADDRESS, "erc20").unwrap();

        // This monitor is specific to an address and cares about decoded calldata, but
        // not logs.
        let monitor = MonitorBuilder::new()
            .id(1)
            .address(&CONTRACT_ADDRESS.to_checksum(None))
            .abi("erc20")
            .filter_script(
                r#"decoded_call.name == "transfer" && decoded_call.params._value > bigint(1000)"#,
            )
            .actions(vec!["test-action".to_string()])
            .build();
        let engine = setup_engine_with_monitors(vec![monitor], abi_service.clone());

        // This transaction matches the monitor's script.
        let tx = TransactionBuilder::new()
            .to(Some(CONTRACT_ADDRESS))
            .input(TX_INPUT_DATA.parse().unwrap())
            .build();
        let item = CorrelatedBlockItem::new(tx, vec![], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1, "Should find one match for high-value transfer");
        assert_eq!(matches[0].monitor_id, 1);
    }

    #[tokio::test]
    async fn test_evaluate_item_log_aware_monitor_with_decoded_call_match() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json())]);
        abi_service.link_abi(CONTRACT_ADDRESS, "erc20").unwrap();

        // This monitor cares about both logs and decoded calldata.
        let monitor = MonitorBuilder::new()
            .id(1)
            .address(&CONTRACT_ADDRESS.to_checksum(None))
            .abi("erc20")
            .filter_script(
                r#"log.name == "Transfer" && decoded_call.name == "transfer" && decoded_call.params._value > bigint(1000)"#,
            )
            .actions(vec!["test-action".to_string()])
            .build();
        let engine = setup_engine_with_monitors(vec![monitor], abi_service.clone());

        // This transaction matches the monitor's script.
        let tx = TransactionBuilder::new()
            .to(Some(CONTRACT_ADDRESS))
            .input(TX_INPUT_DATA.parse().unwrap())
            .build();
        let amount_data = U256::from(1500).abi_encode().into();
        let (_, log) = create_test_log_and_tx_with_topics(
            CONTRACT_ADDRESS,
            vec![TRANSFER_EVENT_TOPIC, FROM_TOPIC, TO_TOPIC],
            amount_data,
        );
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1, "Should find one match for high-value transfer with log");
        assert_eq!(matches[0].monitor_id, 1);
    }

    #[tokio::test]
    async fn test_decoded_call_is_null_for_non_matching_selector() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json())]);
        abi_service.link_abi(CONTRACT_ADDRESS, "erc20").unwrap();

        let monitor = MonitorBuilder::new()
            .id(1)
            .filter_script("decoded_call.name == \"\"") // Check for empty decoded_call name
            .actions(vec!["action1".to_string()])
            .build();
        let monitors = vec![monitor];
        let engine = setup_engine_with_monitors(monitors, abi_service);

        // This transaction has a `to` address, so decoding will be attempted, but
        // the selector is invalid, so it will fail.
        let tx = TransactionBuilder::new()
            .to(Some(CONTRACT_ADDRESS))
            .input(b256!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").into())
            .build();
        let item = CorrelatedBlockItem::new(tx, vec![], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1, "Should match when decoded_call is null");
    }

    #[tokio::test]
    async fn test_requires_receipt_data_flag_set_correctly() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json())]);
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
            .filter_script("log.name == \"Transfer\" ") // No receipt needed
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
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json())]);
        let monitor = MonitorBuilder::new()
            .id(1)
            .filter_script("tx.value > ether(1.5)")
            .actions(vec!["action1".to_string()])
            .build();
        let monitors = vec![monitor];
        let engine = setup_engine_with_monitors(monitors, abi_service);

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
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json())]);

        let addr1 = address!("1111111111111111111111111111111111111111");
        let addr2 = address!("2222222222222222222222222222222222222222");

        // Link ABIs to the addresses so logs can be decoded
        abi_service.link_abi(addr1, "erc20").unwrap();
        abi_service.link_abi(addr2, "erc20").unwrap();

        // This monitor has no address, so it should run on logs from ANY address.
        let global_monitor = MonitorBuilder::new()
            .id(100)
            .abi("erc20")
            .filter_script("log.name == \"Transfer\" ")
            .actions(vec!["global_action".to_string()])
            .build();

        let monitors = vec![global_monitor];
        let engine = setup_engine_with_monitors(monitors, abi_service);

        let amount_data: Bytes = U256::from(1000).abi_encode().into();
        let (tx, log1) = create_test_log_and_tx_with_topics(
            addr1,
            vec![TRANSFER_EVENT_TOPIC, FROM_TOPIC, TO_TOPIC],
            amount_data.clone(),
        );
        let (_, log2) = create_test_log_and_tx_with_topics(
            addr2,
            vec![TRANSFER_EVENT_TOPIC, FROM_TOPIC, TO_TOPIC],
            amount_data,
        );
        // This log should be ignored by the monitor
        let value_transfered_topic =
            b256!("1dd763d000642c1a04c2286c7b36731314905d0623c408543a35b0a50344c66a");
        let (_, log3) = create_test_log_and_tx_with_topics(
            addr1,
            vec![value_transfered_topic],
            Bytes::default(),
        );

        let item = CorrelatedBlockItem::new(tx, vec![log1.clone(), log2.clone(), log3], None);

        let matches = engine.evaluate_item(&item).await.unwrap();

        // We expect two matches, one for each "Transfer" log.
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
            .abi("erc20")
            .filter_script(
                r#" 
            tx.value > bigint(100) || log.name == "Transfer"
        "#,
            )
            .actions(vec!["test-action".to_string()])
            .build();
        let engine = setup_engine_with_monitors(vec![monitor], abi_service.clone());

        // This transaction matches the `tx.value` part of the script and has NO logs.
        let tx = TransactionBuilder::new().value(U256::from(150)).build();
        let item = CorrelatedBlockItem::new(tx, vec![], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1, "Should match on tx.value even with no logs");
        assert_eq!(matches[0].monitor_id, 1);
        assert!(matches!(matches[0].match_data, MatchData::Transaction(_)));
    }

    #[tokio::test]
    async fn test_evaluate_item_hybrid_monitor_log_match_only() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json())]);
        abi_service.link_abi(CONTRACT_ADDRESS, "erc20").unwrap();

        let monitor = MonitorBuilder::new()
            .id(1)
            .abi("erc20")
            .filter_script(
                r#" 
            tx.value > bigint(100) || log.name == "Transfer"
        "#,
            )
            .actions(vec!["test-action".to_string()])
            .build();
        let engine = setup_engine_with_monitors(vec![monitor], abi_service.clone());

        // This transaction does NOT match the tx.value, but its log does.
        let tx = TransactionBuilder::new().value(U256::from(50)).build();
        let amount_data = U256::from(1000).abi_encode().into();
        let (_, log) = create_test_log_and_tx_with_topics(
            CONTRACT_ADDRESS,
            vec![TRANSFER_EVENT_TOPIC, FROM_TOPIC, TO_TOPIC],
            amount_data,
        );
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1, "Should match on log.name");
        assert_eq!(matches[0].monitor_id, 1);
        assert!(matches!(matches[0].match_data, MatchData::Log(_)));
    }

    #[tokio::test]
    async fn test_evaluate_item_hybrid_monitor_prefers_log_match() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json())]);
        abi_service.link_abi(CONTRACT_ADDRESS, "erc20").unwrap();

        let monitor = MonitorBuilder::new()
            .id(1)
            .abi("erc20")
            .filter_script(
                r#" 
            tx.value > bigint(100) || log.name == "Transfer"
        "#,
            )
            .actions(vec!["test-action".to_string()])
            .build();
        let engine = setup_engine_with_monitors(vec![monitor], abi_service.clone());

        // This transaction matches BOTH the tx.value and the log.name.
        let tx = TransactionBuilder::new().value(U256::from(150)).build();
        let amount_data = U256::from(1000).abi_encode().into();
        let (_, log) = create_test_log_and_tx_with_topics(
            CONTRACT_ADDRESS,
            vec![TRANSFER_EVENT_TOPIC, FROM_TOPIC, TO_TOPIC],
            amount_data,
        );
        let item = CorrelatedBlockItem::new(tx, vec![log], None);

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
            .filter_script(r#"decoded_call.name == "nonexistent""#)
            .actions(vec!["test-action".to_string()])
            .build();
        let engine = setup_engine_with_monitors(vec![monitor], abi_service.clone());

        // This item has no decoded_call, so the variable will be `()`.
        let tx = TransactionBuilder::new().build();
        let item = CorrelatedBlockItem::new(tx, vec![], None);

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
            .filter_script(r#"log.name == "nonexistent""#)
            .actions(vec!["test-action".to_string()])
            .build();
        let engine = setup_engine_with_monitors(vec![monitor], abi_service.clone());

        // This item has no logs, so `log` will be `()` during the tx-only pass.
        let tx = TransactionBuilder::new().build();
        let item = CorrelatedBlockItem::new(tx, vec![], None);

        // The script should evaluate to `false` and not error.
        let matches = engine.evaluate_item(&item).await.unwrap();
        assert!(matches.is_empty(), "Should not match and should not error");
    }

    #[tokio::test]
    async fn test_safe_null_access_on_decoded_call_with_valid_call() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json())]);
        abi_service.link_abi(CONTRACT_ADDRESS, "erc20").unwrap();

        let monitor = MonitorBuilder::new()
            .id(1)
            .address(&CONTRACT_ADDRESS.to_checksum(None))
            .abi("erc20")
            .filter_script(r#"decoded_call.name == "transfer""#)
            .actions(vec!["test-action".to_string()])
            .build();
        let engine = setup_engine_with_monitors(vec![monitor], abi_service.clone());

        // This transaction matches the monitor's script.
        let tx = TransactionBuilder::new()
            .to(Some(CONTRACT_ADDRESS))
            .input(TX_INPUT_DATA.parse().unwrap())
            .build();
        let item = CorrelatedBlockItem::new(tx, vec![], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1, "Should find one match for the transfer call");
        assert_eq!(matches[0].monitor_id, 1);
    }

    #[tokio::test]
    async fn test_create_log_matches_payload() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]);
        let engine = setup_engine_with_monitors(vec![], abi_service);

        let tx = TransactionBuilder::new().value(U256::from(123)).build();
        let log = LogBuilder::new().log_index(42).build();
        let item = CorrelatedBlockItem::new(tx.clone(), vec![log.clone()], None);
        let mut context = EvaluationContext::new(&item);

        let monitor = MonitorBuilder::new().actions(vec!["n1".to_string()]).build();
        let decoded_log =
            DecodedLog { log: log.into(), name: "TestEvent".to_string(), params: vec![] };

        engine.create_log_matches(&mut context, &monitor, &decoded_log);

        assert_eq!(context.matches.len(), 1);
        let monitor_match = &context.matches[0];

        match &monitor_match.match_data {
            MatchData::Log(log_match) => {
                // Verify log details
                assert_eq!(log_match.log_details.name, "TestEvent");
                assert_eq!(log_match.log_details.log_index, 42);

                // Verify transaction details
                let tx_details_map = log_match.tx_details.as_object().unwrap();
                assert_eq!(tx_details_map.get("value").unwrap().as_str().unwrap(), "123");
                assert_eq!(
                    tx_details_map.get("hash").unwrap().as_str().unwrap(),
                    tx.hash().to_string()
                );
            }
            _ => panic!("Expected a log match"),
        }
    }

    #[tokio::test]
    async fn test_create_tx_matches_payload() {
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]);
        let engine = setup_engine_with_monitors(vec![], abi_service);

        let tx = TransactionBuilder::new().value(U256::from(456)).build();
        let item = CorrelatedBlockItem::new(tx.clone(), vec![], None);
        let mut context = EvaluationContext::new(&item);

        let monitor = MonitorBuilder::new().actions(vec!["n1".to_string()]).build();

        engine.create_tx_matches(&mut context, &monitor);

        assert_eq!(context.matches.len(), 1);
        let monitor_match = &context.matches[0];

        match &monitor_match.match_data {
            MatchData::Transaction(tx_match) => {
                let tx_details_map = tx_match.details.as_object().unwrap();
                assert_eq!(tx_details_map.get("value").unwrap().as_str().unwrap(), "456");
                assert_eq!(
                    tx_details_map.get("hash").unwrap().as_str().unwrap(),
                    tx.hash().to_string()
                );
            }
            _ => panic!("Expected a transaction match"),
        }
    }
}
