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
        // TODO: add more limits (timeout, memory usage, etc.)
        engine.set_max_operations(1_000_000);

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

    /// Converts a JSON value to a Rhai Dynamic value.
    fn json_to_rhai_dynamic(value: &Value) -> Dynamic {
        match value {
            Value::Null => Dynamic::UNIT,
            Value::Bool(b) => (*b).into(),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    i.into()
                } else if let Some(u) = n.as_u64() {
                    (u as i64).into()
                } else if let Some(f) = n.as_f64() {
                    f.into()
                } else {
                    n.to_string().into()
                }
            }
            Value::String(s) => s.clone().into(),
            Value::Array(arr) => {
                let rhai_array: Vec<Dynamic> = arr.iter().map(Self::json_to_rhai_dynamic).collect();
                rhai_array.into()
            }
            Value::Object(obj) => {
                let mut rhai_map = Map::new();
                for (k, v) in obj {
                    rhai_map.insert(k.clone().into(), Self::json_to_rhai_dynamic(v));
                }
                rhai_map.into()
            }
        }
    }

    /// Builds a Rhai `Map` from a decoded log and its JSON representation.
    fn build_log_map(log: &DecodedLog, trigger_data: &Value) -> Map {
        let mut log_map = Map::new();
        log_map.insert("name".into(), log.name.clone().into());
        log_map.insert("params".into(), Self::json_to_rhai_dynamic(trigger_data));
        log_map
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
        if self.monitors_by_address.is_empty() {
            return Ok(matches);
        }

        // Build a transaction map for the item to access transaction data in the script
        let tx_map = Self::build_tx_map(item.transaction);

        // Iterate over decoded logs in the item
        for log in &item.decoded_logs {
            let log_address_str = log.log.address().to_checksum(None);

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
        DynSolValue::Address(a) => Value::String(a.to_checksum(None)),
        DynSolValue::Bool(b) => Value::Bool(*b),
        DynSolValue::Bytes(b) => Value::String(format!("0x{}", hex::encode(b))),
        DynSolValue::FixedBytes(fb, _) => Value::String(format!("0x{}", hex::encode(fb))),
        // Large signed integers: JSON number if fits in i64, otherwise string
        DynSolValue::Int(i, _) => i
            .to_string()
            .parse::<i64>()
            .map_or_else(|_| Value::String(i.to_string()), Value::from),
        DynSolValue::String(s) => Value::String(s.clone()),
        // Large unsigned integers: JSON number if fits in u64, otherwise string
        DynSolValue::Uint(u, _) => u
            .to_string()
            .parse::<u64>()
            .map_or_else(|_| Value::String(u.to_string()), Value::from),
        DynSolValue::Array(arr) | DynSolValue::FixedArray(arr) | DynSolValue::Tuple(arr) => {
            Value::Array(arr.iter().map(dyn_sol_value_to_json).collect())
        }
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{LogBuilder, TransactionBuilder};
    use alloy::dyn_abi::DynSolValue;
    use alloy::primitives::{Address, I256, U256, address, b256};
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
        let engine =
            RhaiFilteringEngine::new(vec![monitor1.clone(), monitor2.clone(), monitor3.clone()]);

        assert_eq!(engine.monitors_by_address.len(), 2);
        assert_eq!(engine.monitors_by_address.get(addr1).unwrap().len(), 2);
        assert_eq!(engine.monitors_by_address.get(addr2).unwrap().len(), 1);

        // Test `update_monitors()`
        let monitor4 = create_test_monitor(4, addr2, "true");
        engine
            .update_monitors(vec![monitor1.clone(), monitor4.clone()])
            .await;

        assert_eq!(engine.monitors_by_address.len(), 2);
        assert_eq!(engine.monitors_by_address.get(addr1).unwrap().len(), 1);
        assert_eq!(engine.monitors_by_address.get(addr2).unwrap().len(), 1);
        assert_eq!(engine.monitors_by_address.get(addr2).unwrap()[0].id, 4);
    }

    #[tokio::test]
    async fn test_evaluate_item_simple_match() {
        let addr = address!("0000000000000000000000000000000000000001");
        let monitor = create_test_monitor(1, &addr.to_checksum(None), "log.name == \"Transfer\"");
        let engine = RhaiFilteringEngine::new(vec![monitor]);

        let (tx, log) = create_test_log_and_tx(addr, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(&tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].trigger_name, "Transfer");
    }

    #[tokio::test]
    async fn test_evaluate_item_no_monitors() {
        let engine = RhaiFilteringEngine::new(vec![]); // No monitors
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
        let engine = RhaiFilteringEngine::new(vec![monitor]);

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
        let engine = RhaiFilteringEngine::new(vec![monitor]);

        // Create a log for addr2
        let (tx, log) = create_test_log_and_tx(addr2, "Transfer", vec![]);
        let item = CorrelatedBlockItem::new(&tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert!(matches.is_empty());
    }

    #[test]
    fn test_dyn_sol_value_to_json() {
        // Address
        let addr = Address::repeat_byte(0x11);
        assert_eq!(
            dyn_sol_value_to_json(&DynSolValue::Address(addr)),
            json!("0x1111111111111111111111111111111111111111")
        );

        // Bool
        assert_eq!(dyn_sol_value_to_json(&DynSolValue::Bool(true)), json!(true));

        // Bytes
        let bytes = vec![0x01, 0x02, 0x03];
        assert_eq!(
            dyn_sol_value_to_json(&DynSolValue::Bytes(bytes.clone().into())),
            json!("0x010203")
        );

        // FixedBytes
        let fixed_bytes = b256!("0405060000000000000000000000000000000000000000000000000000000000");
        assert_eq!(
            dyn_sol_value_to_json(&DynSolValue::FixedBytes(fixed_bytes, 32)),
            json!("0x0405060000000000000000000000000000000000000000000000000000000000")
        );

        // Int
        assert_eq!(
            dyn_sol_value_to_json(&DynSolValue::Int(I256::try_from(123i64).unwrap(), 256)),
            json!(123)
        );
        assert_eq!(
            dyn_sol_value_to_json(&DynSolValue::Int(I256::try_from(-1i64).unwrap(), 256)),
            json!(-1)
        );

        // String
        assert_eq!(
            dyn_sol_value_to_json(&DynSolValue::String("hello".to_string())),
            json!("hello")
        );

        // Uint
        let uint_val = U256::from(456);
        assert_eq!(
            dyn_sol_value_to_json(&DynSolValue::Uint(uint_val.into(), 256)),
            json!(456)
        );

        // Array
        let arr = vec![
            DynSolValue::Uint(U256::from(1).into(), 256),
            DynSolValue::Bool(false),
        ];
        assert_eq!(
            dyn_sol_value_to_json(&DynSolValue::Array(arr)),
            json!([1, false])
        );

        // FixedArray
        let fixed_arr = vec![
            DynSolValue::Uint(U256::from(10).into(), 256),
            DynSolValue::Uint(U256::from(20).into(), 256),
        ];
        assert_eq!(
            dyn_sol_value_to_json(&DynSolValue::FixedArray(fixed_arr)),
            json!([10, 20])
        );

        // Tuple
        let tuple = vec![
            DynSolValue::String("test".to_string()),
            DynSolValue::Int(I256::try_from(789i64).unwrap(), 256),
        ];
        assert_eq!(
            dyn_sol_value_to_json(&DynSolValue::Tuple(tuple)),
            json!(["test", 789])
        );

        // Null/Other
        assert_eq!(
            dyn_sol_value_to_json(&DynSolValue::Bytes(vec![].into())),
            json!("0x")
        );
    }

    #[test]
    fn test_dyn_sol_value_to_json_large_numbers() {
        // Test values at i64 boundaries
        let i64_max = I256::try_from(i64::MAX).unwrap();
        assert_eq!(
            dyn_sol_value_to_json(&DynSolValue::Int(i64_max, 256)),
            json!(i64::MAX)
        );

        let i64_min = I256::try_from(i64::MIN).unwrap();
        assert_eq!(
            dyn_sol_value_to_json(&DynSolValue::Int(i64_min, 256)),
            json!(i64::MIN)
        );

        // Test values beyond i64 range (should become strings)
        let beyond_i64_max = I256::try_from(i64::MAX).unwrap() + I256::try_from(1).unwrap();
        let result = dyn_sol_value_to_json(&DynSolValue::Int(beyond_i64_max, 256));
        assert!(result.is_string());
        assert_eq!(result.as_str().unwrap(), beyond_i64_max.to_string());

        let beyond_i64_min = I256::try_from(i64::MIN).unwrap() - I256::try_from(1).unwrap();
        let result = dyn_sol_value_to_json(&DynSolValue::Int(beyond_i64_min, 256));
        assert!(result.is_string());
        assert_eq!(result.as_str().unwrap(), beyond_i64_min.to_string());

        // Test values at u64 boundaries for Uint
        let u64_max = U256::from(u64::MAX);
        assert_eq!(
            dyn_sol_value_to_json(&DynSolValue::Uint(u64_max.into(), 256)),
            json!(u64::MAX)
        );

        // Test values beyond u64 range (should become strings)
        let beyond_u64_max = U256::from(u64::MAX) + U256::from(1);
        let result = dyn_sol_value_to_json(&DynSolValue::Uint(beyond_u64_max.into(), 256));
        assert!(result.is_string());
        assert_eq!(result.as_str().unwrap(), beyond_u64_max.to_string());

        // Test very large numbers (close to U256::MAX)
        let very_large = U256::MAX - U256::from(1);
        let result = dyn_sol_value_to_json(&DynSolValue::Uint(very_large.into(), 256));
        assert!(result.is_string());
        assert_eq!(result.as_str().unwrap(), very_large.to_string());

        // Test U256::MAX itself
        let result = dyn_sol_value_to_json(&DynSolValue::Uint(U256::MAX.into(), 256));
        assert!(result.is_string());
        assert_eq!(result.as_str().unwrap(), U256::MAX.to_string());
    }

    #[tokio::test]
    async fn test_evaluate_item_multiple_monitors_same_address() {
        let addr = address!("0000000000000000000000000000000000000001");
        // Two monitors for the same address with the same event name
        let monitor1 = create_test_monitor(1, &addr.to_checksum(None), "log.name == \"Transfer\"");
        let monitor2 = create_test_monitor(2, &addr.to_checksum(None), "log.name == \"Transfer\"");
        let engine = RhaiFilteringEngine::new(vec![monitor1, monitor2]);

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
        let engine = RhaiFilteringEngine::new(vec![monitor]);

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
}
