//! This module defines the `FilteringEngine` and its implementations.

use crate::abi::DecodedLog;
use crate::models::correlated_data::CorrelatedBlockItem;
use crate::models::monitor::Monitor;
use crate::models::monitor_match::MonitorMatch;
use crate::models::transaction::Transaction;
use alloy::dyn_abi::DynSolValue;
use async_trait::async_trait;
use dashmap::DashMap;
#[cfg(test)]
use mockall::automock;
use rhai::{Dynamic, Engine, Map, Scope};
use serde_json::Value;
use std::sync::Arc;

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
}

/// A Rhai-based implementation of the `FilteringEngine`.
#[derive(Debug)]
pub struct RhaiFilteringEngine {
    monitors_by_address: Arc<DashMap<String, Vec<Monitor>>>,
    engine: Engine,
}

impl RhaiFilteringEngine {
    /// Creates a new `RhaiFilteringEngine`.
    pub fn new(monitors: Vec<Monitor>) -> Self {
        let mut engine = Engine::new();
        engine.set_max_operations(1_000_000); // Prevent runaway scripts

        let monitors_by_address: DashMap<String, Vec<Monitor>> = DashMap::new();
        for monitor in monitors {
            monitors_by_address
                .entry(monitor.address.clone())
                .or_default()
                .push(monitor);
        }

        Self {
            monitors_by_address: Arc::new(monitors_by_address),
            engine,
        }
    }

    /// Builds a Rhai `Map` from a transaction.
    fn build_tx_map(transaction: &Transaction) -> Map {
        let mut map = Map::new();
        if let Some(to) = transaction.to() {
            map.insert("to".into(), to.to_string().into());
        }
        map.insert("from".into(), transaction.from().to_string().into());
        map.insert("value".into(), transaction.value().to_string().into());
        map.insert("hash".into(), transaction.hash().to_string().into());
        map
    }

    /// Builds a Rhai `Map` from a decoded log and its JSON representation.
    fn build_log_map(log: &DecodedLog, trigger_data: &Value) -> Map {
        let mut log_map = Map::new();
        log_map.insert("name".into(), log.name.clone().into());
        log_map.insert("params".into(), Dynamic::from(trigger_data.clone()));
        log_map
    }
}

#[async_trait]
impl FilteringEngine for RhaiFilteringEngine {
    #[tracing::instrument(skip(self, item))]
    async fn evaluate_item<'a>(
        &self,
        item: &CorrelatedBlockItem<'a>,
    ) -> Result<Vec<MonitorMatch>, Box<dyn std::error::Error + Send + Sync>> {
        let mut matches = Vec::new();
        
        // If no monitors are configured, return early
        if self.monitors_by_address.is_empty() {
            return Ok(matches);
        }

        // Build a transaction map for the item to access transaction data in the script
        let tx_map = Self::build_tx_map(item.transaction);

        // Iterate over decoded logs in the item
        for log in &item.decoded_logs {
            let log_address_str = format!("{:?}", log.log.address());

            // Efficiently look up monitors for the current log's address
            if let Some(monitors) = self.monitors_by_address.get(&log_address_str) {
                for monitor in monitors.iter() {
                    // Build trigger data from log parameters
                    let trigger_data = {
                        let mut data = serde_json::Map::new();
                        for (name, value) in &log.params {
                            data.insert(name.clone(), dyn_sol_value_to_json(value));
                        }
                        Value::Object(data)
                    };

                    // Create a new scope for the monitor evaluation
                    let mut scope = Scope::new();
                    scope.push("tx", tx_map.clone());
                    scope.push("log", Self::build_log_map(log, &trigger_data));

                    // Compile and evaluate the monitor's filter script
                    let ast = match self.engine.compile(&monitor.filter_script) {
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

                    // Evaluate the script with the current scope
                    match self.engine.eval_ast_with_scope::<bool>(&mut scope, &ast) {
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
                                "Failed to evaluate script: {}",
                                e
                            );
                        }
                    }
                }
            }
        }

        Ok(matches)
    }

    /// Updates the set of monitors used by the engine.
    async fn update_monitors(&self, monitors: Vec<Monitor>) {
        self.monitors_by_address.clear();
        for monitor in monitors {
            self.monitors_by_address
                .entry(monitor.address.clone())
                .or_default()
                .push(monitor);
        }
    }
}

/// Converts a `DynSolValue` to a `serde_json::Value`.
fn dyn_sol_value_to_json(value: &DynSolValue) -> Value {
    match value {
        DynSolValue::Address(a) => Value::String(format!("{a:?}")),
        DynSolValue::Bool(b) => Value::Bool(*b),
        DynSolValue::Bytes(b) => Value::String(format!("0x{}", hex::encode(b))),
        DynSolValue::FixedBytes(fb, _) => Value::String(format!("0x{}", hex::encode(fb))),
        DynSolValue::Int(i, _) => Value::String(i.to_string()),
        DynSolValue::String(s) => Value::String(s.clone()),
        DynSolValue::Uint(u, _) => Value::String(u.to_string()),
        DynSolValue::Array(arr) | DynSolValue::Tuple(arr) => {
            Value::Array(arr.iter().map(dyn_sol_value_to_json).collect())
        }
        _ => Value::Null,
    }
}
