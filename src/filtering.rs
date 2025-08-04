//! This module defines the `FilteringEngine` and its implementations.

use crate::abi::DecodedLog;
use crate::models::correlated_data::CorrelatedBlockItem;
use crate::models::monitor::Monitor;
use crate::models::monitor_match::MonitorMatch;
use crate::models::transaction::Transaction;
use alloy::dyn_abi::DynSolValue;
use alloy::primitives::U256;
use async_trait::async_trait;
use dashmap::DashMap;
#[cfg(test)]
use mockall::automock;
use rhai::{Engine, Map, Scope};
use serde_json::Value;
use std::{str::FromStr, sync::Arc};

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
        
        // Register custom functions for big number handling
        engine.register_fn("parse_uint", |s: &str| -> Result<String, Box<rhai::EvalAltResult>> {
            match U256::from_str(s) {
                Ok(val) => {
                    // Always return as padded string for consistent comparison
                    Ok(format!("{:078}", val))
                },
                Err(_) => Err(format!("Failed to parse uint: {}", s).into()),
            }
        });

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
        log_map.insert("params".into(), serde_json::from_value(trigger_data.clone()).unwrap());
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
/// For Int/Uint values, tries to fit them into native JSON numbers (i64/u64),
/// then falls back to i128/u128, and finally to string representation for larger values.
fn dyn_sol_value_to_json(value: &DynSolValue) -> Value {
    match value {
        DynSolValue::Address(a) => Value::String(format!("{a:?}")),
        DynSolValue::Bool(b) => Value::Bool(*b),
        DynSolValue::Bytes(b) => Value::String(format!("0x{}", hex::encode(b))),
        DynSolValue::FixedBytes(fb, _) => Value::String(format!("0x{}", hex::encode(fb))),
        DynSolValue::Int(i, _) => {
            // Try i64 first (for JSON compatibility), then i128, then string
            let string_val = i.to_string();
            if let Ok(val) = string_val.parse::<i64>() {
                Value::from(val)
            } else if let Ok(val) = string_val.parse::<i128>() {
                // serde_json doesn't directly support i128, but we can represent it as a number
                // if it fits in f64 range without precision loss, otherwise as string
                if val >= -(2i128.pow(53)) && val <= 2i128.pow(53) {
                    Value::Number(serde_json::Number::from(val as i64))
                } else {
                    Value::String(string_val)
                }
            } else {
                Value::String(string_val)
            }
        },
        DynSolValue::String(s) => Value::String(s.clone()),
        DynSolValue::Uint(u, _) => {
            // Try u64 first (for JSON compatibility), then u128, then string
            let string_val = u.to_string();
            if let Ok(val) = string_val.parse::<u64>() {
                Value::from(val)
            } else if let Ok(val) = string_val.parse::<u128>() {
                // serde_json doesn't directly support u128, but we can represent it as a number
                // if it fits in f64 range without precision loss, otherwise as string
                if val <= 2u128.pow(53) {
                    Value::Number(serde_json::Number::from(val as u64))
                } else {
                    Value::String(string_val)
                }
            } else {
                Value::String(string_val)
            }
        },
        DynSolValue::Array(arr) | DynSolValue::FixedArray(arr)| DynSolValue::Tuple(arr) => {
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
        let monitor = create_test_monitor(1, &format!("{addr:?}"), "log.name == \"Transfer\"");
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
        let monitor = create_test_monitor(1, &format!("{addr:?}"), "log.name == \"AnotherEvent\"");
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
        let monitor = create_test_monitor(1, &format!("{addr1:?}"), "true");
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
    fn test_dyn_sol_value_to_json_number_handling() {
        // Test small numbers that fit in i64/u64 - should be JSON numbers
        let small_uint = DynSolValue::Uint(U256::from(42).into(), 256);
        let small_int = DynSolValue::Int(I256::try_from(42).unwrap().into(), 256);
        let negative_int = DynSolValue::Int(I256::try_from(-42).unwrap().into(), 256);
        
        assert_eq!(dyn_sol_value_to_json(&small_uint), json!(42));
        assert_eq!(dyn_sol_value_to_json(&small_int), json!(42));
        assert_eq!(dyn_sol_value_to_json(&negative_int), json!(-42));
        
        // Test maximum i64/u64 values - should still be JSON numbers
        let max_u64 = DynSolValue::Uint(U256::from(u64::MAX).into(), 256);
        let max_i64 = DynSolValue::Int(I256::try_from(i64::MAX).unwrap().into(), 256);
        let min_i64 = DynSolValue::Int(I256::try_from(i64::MIN).unwrap().into(), 256);
        
        assert_eq!(dyn_sol_value_to_json(&max_u64), json!(u64::MAX));
        assert_eq!(dyn_sol_value_to_json(&max_i64), json!(i64::MAX));
        assert_eq!(dyn_sol_value_to_json(&min_i64), json!(i64::MIN));
        
        // Test values that exceed i64/u64 but fit in safe JSON range (2^53) - should be JSON numbers
        let large_safe_uint = DynSolValue::Uint(U256::from(2u64.pow(53)).into(), 256);
        let large_safe_int = DynSolValue::Int(I256::try_from(2i64.pow(53)).unwrap().into(), 256);
        
        assert_eq!(dyn_sol_value_to_json(&large_safe_uint), json!(2u64.pow(53)));
        assert_eq!(dyn_sol_value_to_json(&large_safe_int), json!(2i64.pow(53)));
        
        // Test very large numbers that exceed safe JSON range - should be strings
        let very_large_uint = DynSolValue::Uint(U256::MAX.into(), 256);
        let very_large_int = DynSolValue::Int(I256::MAX.into(), 256);
        let very_small_int = DynSolValue::Int(I256::MIN.into(), 256);
        
        assert_eq!(dyn_sol_value_to_json(&very_large_uint), json!(U256::MAX.to_string()));
        assert_eq!(dyn_sol_value_to_json(&very_large_int), json!(I256::MAX.to_string()));
        assert_eq!(dyn_sol_value_to_json(&very_small_int), json!(I256::MIN.to_string()));
    }

    #[tokio::test]
    async fn test_evaluate_item_multiple_monitors_same_address() {
        let addr = address!("0000000000000000000000000000000000000001");
        // Two monitors for the same address with the same event name
        let monitor1 = create_test_monitor(1, &format!("{addr:?}"), "log.name == \"Transfer\"");
        let monitor2 = create_test_monitor(2, &format!("{addr:?}"), "log.name == \"Transfer\"");
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
            &format!("{addr:?}"),
            "log.name == \"ValueTransfered\" && log.params.value > 100",
        );
        let engine = RhaiFilteringEngine::new(vec![monitor]);

        let (tx, log) = create_test_log_and_tx(
            addr,
            "ValueTransfered",
            vec![("value".to_string(), DynSolValue::Uint(U256::from(150).into(), 256))],
        );
        let item = CorrelatedBlockItem::new(&tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].trigger_name, "ValueTransfered");
        assert_eq!(matches[0].trigger_data["value"], json!(150));
    }

    #[tokio::test]
    async fn test_evaluate_item_filter_by_large_log_param() {
        let addr = address!("0000000000000000000000000000000000000001");
        let large_value = U256::MAX; 
        
        let monitor = create_test_monitor(
            1,
            &format!("{addr:?}"),
            &format!("log.name == \"LargeValueTransfered\" && parse_uint(log.params.value) > parse_uint(\"{}\")", U256::from(0)),
        );
        let engine = RhaiFilteringEngine::new(vec![monitor]);

        let (tx, log) = create_test_log_and_tx(
            addr,
            "LargeValueTransfered",
            vec![("value".to_string(), DynSolValue::Uint(large_value.into(), 256))],
        );
        let item = CorrelatedBlockItem::new(&tx, vec![log], None);

        let matches = engine.evaluate_item(&item).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].monitor_id, 1);
        assert_eq!(matches[0].trigger_name, "LargeValueTransfered");
        assert_eq!(matches[0].trigger_data["value"], json!(large_value.to_string()));
    }
}
