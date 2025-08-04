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

/// Converts a `DynSolValue` directly to a `serde_json::Value`.
///
/// Large numbers (>i64/u64 range) are converted to strings to preserve precision.
pub fn dyn_sol_value_to_json(value: &DynSolValue) -> Value {
    match value {
        DynSolValue::Address(a) => Value::String(a.to_checksum(None)),
        DynSolValue::Bool(b) => Value::Bool(*b),
        DynSolValue::Bytes(b) => Value::String(format!("0x{}", hex::encode(b))),
        DynSolValue::FixedBytes(fb, _) => Value::String(format!("0x{}", hex::encode(fb))),

        // Handle large signed integers
        DynSolValue::Int(i, _) => {
            // Try to fit in i64, otherwise use string
            match i.to_string().parse::<i64>() {
                Ok(val) => Value::Number(serde_json::Number::from(val)),
                Err(_) => Value::String(i.to_string()),
            }
        }

        DynSolValue::String(s) => Value::String(s.clone()),

        // Handle large unsigned integers
        DynSolValue::Uint(u, _) => {
            // Try to fit in i64, otherwise use string
            let u_str = u.to_string();
            match u_str.parse::<i64>() {
                Ok(val) => Value::Number(serde_json::Number::from(val)),
                Err(_) => Value::String(u_str),
            }
        }

        // Handle arrays and tuples
        DynSolValue::Array(arr) | DynSolValue::FixedArray(arr) | DynSolValue::Tuple(arr) => {
            let json_array: Vec<Value> = arr.iter().map(dyn_sol_value_to_json).collect();
            Value::Array(json_array)
        }

        _ => Value::Null,
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

/// Builds trigger data JSON from log parameters using the same conversion logic as Rhai.
///
/// This ensures consistency between the data seen by Rhai scripts and the data in trigger_data.
pub fn build_trigger_data_from_params(params: &[(String, DynSolValue)]) -> Value {
    let mut json_map = serde_json::Map::new();
    for (name, value) in params {
        json_map.insert(name.clone(), dyn_sol_value_to_json(value));
    }
    Value::Object(json_map)
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
    fn test_dyn_sol_value_to_json_basic_types() {
        // Address
        let addr = Address::repeat_byte(0x11);
        let result = dyn_sol_value_to_json(&DynSolValue::Address(addr));
        assert_eq!(result, json!("0x1111111111111111111111111111111111111111"));

        // Bool
        let result = dyn_sol_value_to_json(&DynSolValue::Bool(true));
        assert_eq!(result, json!(true));

        // String
        let result = dyn_sol_value_to_json(&DynSolValue::String("hello".to_string()));
        assert_eq!(result, json!("hello"));

        // Small Uint (should become number)
        let result = dyn_sol_value_to_json(&DynSolValue::Uint(U256::from(123).into(), 256));
        assert_eq!(result, json!(123));
    }

    #[test]
    fn test_dyn_sol_value_to_json_large_numbers() {
        // Large Uint (should become string)
        let large_uint = U256::MAX;
        let result = dyn_sol_value_to_json(&DynSolValue::Uint(large_uint.into(), 256));
        assert_eq!(result, json!(large_uint.to_string()));

        // Uint at i64::MAX boundary (should stay as number)
        let max_i64_as_u256 = U256::from(i64::MAX as u64);
        let result = dyn_sol_value_to_json(&DynSolValue::Uint(max_i64_as_u256.into(), 256));
        assert_eq!(result, json!(i64::MAX));

        // Uint just beyond i64::MAX (should become string)
        let beyond_i64_max = U256::from(i64::MAX as u64) + U256::from(1);
        let result = dyn_sol_value_to_json(&DynSolValue::Uint(beyond_i64_max.into(), 256));
        assert_eq!(result, json!(beyond_i64_max.to_string()));
    }

    #[test]
    fn test_dyn_sol_value_to_json_arrays() {
        let arr = vec![
            DynSolValue::Uint(U256::from(1).into(), 256),
            DynSolValue::Bool(false),
            DynSolValue::String("test".to_string()),
        ];

        let result = dyn_sol_value_to_json(&DynSolValue::Array(arr));
        assert_eq!(result, json!([1, false, "test"]));
    }

    #[test]
    fn test_build_trigger_data_from_params() {
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

        let result = build_trigger_data_from_params(&params);
        assert_eq!(
            result,
            json!({
                "value": 150,
                "sender": "0x1111111111111111111111111111111111111111"
            })
        );
    }
}
