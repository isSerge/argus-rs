//! Data conversion utilities for Rhai scripting engine.
//!
//! This module handles conversion between blockchain data types and Rhai-compatible types

use crate::abi::DecodedLog;
use crate::models::transaction::Transaction;
use alloy::dyn_abi::DynSolValue;
use alloy::primitives::U256;
use rhai::{Dynamic, Map};
use serde_json::Value;

/// Converts a `DynSolValue` directly to a Rhai `Dynamic` value.
///
/// Large numbers (>i64/u64 range) are converted to strings to preserve precision.
pub fn dyn_sol_value_to_rhai(value: &DynSolValue) -> Dynamic {
    match value {
        DynSolValue::Address(a) => a.to_checksum(None).into(),
        DynSolValue::Bool(b) => (*b).into(),
        DynSolValue::Bytes(b) => format!("0x{}", hex::encode(b)).into(),
        DynSolValue::FixedBytes(fb, _) => format!("0x{}", hex::encode(fb)).into(),

        // Handle large signed integers
        DynSolValue::Int(i, _) => {
            // Try to fit in i64, otherwise use string
            match i.to_string().parse::<i64>() {
                Ok(val) => val.into(),
                Err(_) => i.to_string().into(),
            }
        }

        DynSolValue::String(s) => s.clone().into(),

        // Handle large unsigned integers
        DynSolValue::Uint(u, _) => {
            // Try to fit in i64 (Rhai's native int type), otherwise use string
            let u_str = u.to_string();
            match u_str.parse::<i64>() {
                Ok(val) => val.into(),
                Err(_) => u_str.into(),
            }
        }

        // Handle arrays and tuples
        DynSolValue::Array(arr) | DynSolValue::FixedArray(arr) | DynSolValue::Tuple(arr) => {
            let rhai_array: Vec<Dynamic> = arr.iter().map(dyn_sol_value_to_rhai).collect();
            rhai_array.into()
        }

        _ => Dynamic::UNIT,
    }
}

/// Builds a Rhai `Map` from log parameters using direct conversion.
pub fn build_log_params_map(params: &[(String, DynSolValue)]) -> Map {
    let mut map = Map::new();
    for (name, value) in params {
        map.insert(name.clone().into(), dyn_sol_value_to_rhai(value));
    }
    map
}

/// Builds a Rhai `Map` from a transaction.
pub fn build_transaction_map(transaction: &Transaction) -> Map {
    let mut map = Map::new();

    if let Some(to) = transaction.to() {
        map.insert("to".into(), to.to_checksum(None).into());
    }

    map.insert("from".into(), transaction.from().to_checksum(None).into());
    map.insert("hash".into(), transaction.hash().to_string().into());

    // Convert transaction value directly to appropriate type
    let value_u256 = transaction.value();
    let value_dynamic = convert_u256_to_rhai(value_u256);
    map.insert("value".into(), value_dynamic);

    // TODO: Add more transaction fields as needed:
    // - gas_limit, gas_used, gas_price
    // - block_number, transaction_index
    // - nonce, input data

    map
}

/// Builds a Rhai `Map` from a decoded log.
pub fn build_log_map(log: &DecodedLog, params_map: Map) -> Map {
    let mut log_map = Map::new();
    log_map.insert("name".into(), log.name.clone().into());
    log_map.insert("params".into(), params_map.into());

    // TODO: Add more log fields as needed:
    // - address, block_number, transaction_hash
    // - log_index, transaction_index

    log_map
}

/// Converts a U256 value to the most appropriate Rhai type.
fn convert_u256_to_rhai(value: U256) -> Dynamic {
    // Try to fit in i64 (Rhai's native integer type)
    if value <= U256::from(i64::MAX as u64) {
        (value.to::<u64>() as i64).into()
    } else {
        // Too large for i64, use string representation
        value.to_string().into()
    }
}

/// Legacy function: Converts a JSON value to a Rhai Dynamic value.
///
/// This is kept for backward compatibility but should be avoided in favor
/// of direct conversion functions where possible.
pub fn json_to_rhai_dynamic(value: &Value) -> Dynamic {
    match value {
        Value::Null => Dynamic::UNIT,
        Value::Bool(b) => (*b).into(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.into()
            } else if let Some(u) = n.as_u64() {
                // Check for overflow before casting
                if u <= i64::MAX as u64 {
                    (u as i64).into()
                } else {
                    // Large number, convert to string to preserve value
                    u.to_string().into()
                }
            } else if let Some(f) = n.as_f64() {
                f.into()
            } else {
                n.to_string().into()
            }
        }
        Value::String(s) => s.clone().into(),
        Value::Array(arr) => {
            let rhai_array: Vec<Dynamic> = arr.iter().map(json_to_rhai_dynamic).collect();
            rhai_array.into()
        }
        Value::Object(obj) => {
            let mut rhai_map = Map::new();
            for (k, v) in obj {
                rhai_map.insert(k.clone().into(), json_to_rhai_dynamic(v));
            }
            rhai_map.into()
        }
    }
}

/// Legacy function: Converts a `DynSolValue` to a `serde_json::Value`.
///
/// This is kept for backward compatibility with the trigger_data field
/// in MonitorMatch, but should be avoided for Rhai conversions.
pub fn dyn_sol_value_to_json(value: &DynSolValue) -> Value {
    match value {
        DynSolValue::Address(a) => Value::String(a.to_checksum(None)),
        DynSolValue::Bool(b) => Value::Bool(*b),
        DynSolValue::Bytes(b) => Value::String(format!("0x{}", hex::encode(b))),
        DynSolValue::FixedBytes(fb, _) => Value::String(format!("0x{}", hex::encode(fb))),
        DynSolValue::Int(i, _) => i
            .to_string()
            .parse::<i64>()
            .map_or_else(|_| Value::String(i.to_string()), Value::from),
        DynSolValue::String(s) => Value::String(s.clone()),
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
    use alloy::primitives::{Address, address};
    use serde_json::json;

    #[test]
    fn test_dyn_sol_value_to_rhai_basic_types() {
        // Address
        let addr = Address::repeat_byte(0x11);
        let result = dyn_sol_value_to_rhai(&DynSolValue::Address(addr));
        assert_eq!(
            result.cast::<String>(),
            "0x1111111111111111111111111111111111111111"
        );

        // Bool
        let result = dyn_sol_value_to_rhai(&DynSolValue::Bool(true));
        assert_eq!(result.cast::<bool>(), true);

        // String
        let result = dyn_sol_value_to_rhai(&DynSolValue::String("hello".to_string()));
        assert_eq!(result.cast::<String>(), "hello");

        // Small Uint (should become i64)
        let result = dyn_sol_value_to_rhai(&DynSolValue::Uint(U256::from(123).into(), 256));
        assert_eq!(result.cast::<i64>(), 123);
    }

    #[test]
    fn test_dyn_sol_value_to_rhai_large_numbers() {
        // Large Uint (should become string)
        let large_uint = U256::MAX;
        let result = dyn_sol_value_to_rhai(&DynSolValue::Uint(large_uint.into(), 256));
        assert_eq!(result.cast::<String>(), large_uint.to_string());

        // Uint at i64::MAX boundary (should stay as i64)
        let max_i64_as_u256 = U256::from(i64::MAX as u64);
        let result = dyn_sol_value_to_rhai(&DynSolValue::Uint(max_i64_as_u256.into(), 256));
        assert_eq!(result.cast::<i64>(), i64::MAX);

        // Uint just beyond i64::MAX (should become string)
        let beyond_i64_max = U256::from(i64::MAX as u64) + U256::from(1);
        let result = dyn_sol_value_to_rhai(&DynSolValue::Uint(beyond_i64_max.into(), 256));
        assert_eq!(result.cast::<String>(), beyond_i64_max.to_string());
    }

    #[test]
    fn test_dyn_sol_value_to_rhai_arrays() {
        let arr = vec![
            DynSolValue::Uint(U256::from(1).into(), 256),
            DynSolValue::Bool(false),
            DynSolValue::String("test".to_string()),
        ];

        let result = dyn_sol_value_to_rhai(&DynSolValue::Array(arr));
        let rhai_array = result.cast::<rhai::Array>();

        assert_eq!(rhai_array.len(), 3);
        assert_eq!(rhai_array[0].clone().cast::<i64>(), 1);
        assert_eq!(rhai_array[1].clone().cast::<bool>(), false);
        assert_eq!(rhai_array[2].clone().cast::<String>(), "test");
    }

    #[test]
    fn test_build_log_params_map() {
        let params = vec![
            (
                "value".to_string(),
                DynSolValue::Uint(U256::from(150).into(), 256),
            ),
            (
                "sender".to_string(),
                DynSolValue::Address(address!("1111111111111111111111111111111111111111")),
            ),
        ];

        let result = build_log_params_map(&params);

        assert_eq!(result.len(), 2);
        assert_eq!(result.get("value").unwrap().clone().cast::<i64>(), 150);
        assert_eq!(
            result.get("sender").unwrap().clone().cast::<String>(),
            "0x1111111111111111111111111111111111111111"
        );
    }

    #[test]
    fn test_convert_u256_to_rhai() {
        // Small value
        assert_eq!(convert_u256_to_rhai(U256::from(123)).cast::<i64>(), 123);

        // At boundary
        assert_eq!(
            convert_u256_to_rhai(U256::from(i64::MAX as u64)).cast::<i64>(),
            i64::MAX
        );

        // Beyond boundary
        let large = U256::from(i64::MAX as u64) + U256::from(1);
        assert_eq!(
            convert_u256_to_rhai(large).cast::<String>(),
            large.to_string()
        );
    }

    #[test]
    fn test_json_to_rhai_dynamic_overflow_fix() {
        // Test the overflow fix in json_to_rhai_dynamic
        let large_u64 = (i64::MAX as u64) + 1;
        let json_val = json!(large_u64);

        let result = json_to_rhai_dynamic(&json_val);
        // Should be converted to string to avoid overflow
        assert_eq!(result.cast::<String>(), large_u64.to_string());
    }
}
