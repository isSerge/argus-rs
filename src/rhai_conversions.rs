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

/// Converts a Rhai Dynamic value to JSON for backward compatibility.
///
/// This function enables creating JSON representations from the same data used in Rhai scripts,
/// ensuring consistency between script evaluation and external data formats.
pub fn rhai_dynamic_to_json(dynamic: &Dynamic) -> Value {
    // Handle unit/null values
    if dynamic.is_unit() {
        return Value::Null;
    }

    // Handle primitive types
    if let Some(value) = try_convert_primitive(dynamic) {
        return value;
    }

    // Handle collection types
    if let Some(value) = try_convert_array(dynamic) {
        return value;
    }

    if let Some(value) = try_convert_map(dynamic) {
        return value;
    }

    // Fallback: convert to string representation
    Value::String(dynamic.to_string())
}

/// Attempts to convert a Rhai Dynamic to a JSON primitive type.
fn try_convert_primitive(dynamic: &Dynamic) -> Option<Value> {
    if let Ok(b) = dynamic.as_bool() {
        Some(Value::Bool(b))
    } else if dynamic.is::<i32>() {
        // Handle i32 specifically
        if let Some(i) = dynamic.clone().try_cast::<i32>() {
            Some(Value::Number(serde_json::Number::from(i)))
        } else {
            None
        }
    } else if dynamic.is_int() {
        // Handle other integer types
        if let Ok(i) = dynamic.as_int() {
            Some(Value::Number(serde_json::Number::from(i)))
        } else {
            let value_str = dynamic.to_string();
            if let Ok(i) = value_str.parse::<i64>() {
                Some(Value::Number(serde_json::Number::from(i)))
            } else {
                Some(Value::String(value_str))
            }
        }
    } else if dynamic.is_float() {
        if let Ok(f) = dynamic.as_float() {
            serde_json::Number::from_f64(f).map(Value::Number)
        } else {
            let value_str = dynamic.to_string();
            if let Ok(f) = value_str.parse::<f64>() {
                serde_json::Number::from_f64(f).map(Value::Number)
            } else {
                Some(Value::String(value_str))
            }
        }
    } else if dynamic.is_string() {
        if let Ok(s) = dynamic.clone().into_string() {
            Some(Value::String(s))
        } else {
            Some(Value::String(dynamic.to_string()))
        }
    } else {
        None
    }
}

/// Attempts to convert a Rhai Dynamic array to a JSON array.
fn try_convert_array(dynamic: &Dynamic) -> Option<Value> {
    dynamic.read_lock::<Vec<Dynamic>>().map(|arr| {
        let json_array: Vec<Value> = arr.iter().map(rhai_dynamic_to_json).collect();
        Value::Array(json_array)
    })
}

/// Attempts to convert a Rhai Dynamic map to a JSON object.
fn try_convert_map(dynamic: &Dynamic) -> Option<Value> {
    dynamic.read_lock::<Map>().map(|map| {
        let json_map: serde_json::Map<String, Value> = map
            .iter()
            .map(|(key, value)| (key.to_string(), rhai_dynamic_to_json(value)))
            .collect();
        Value::Object(json_map)
    })
}

/// Builds trigger data JSON from log parameters using the same conversion logic as Rhai.
///
/// This ensures consistency between the data seen by Rhai scripts and the data in trigger_data.
pub fn build_trigger_data_from_params(params: &[(String, DynSolValue)]) -> Value {
    let params_map = build_log_params_map(params);
    rhai_dynamic_to_json(&params_map.into())
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

    #[test]
    fn test_rhai_dynamic_to_json() {
        let dynamic_value = Dynamic::from(123);
        let json_value = rhai_dynamic_to_json(&dynamic_value);
        assert_eq!(json_value, Value::Number(serde_json::Number::from(123)));

        let dynamic_array = Dynamic::from(vec![Dynamic::from(1), Dynamic::from(2)]);
        let json_array = rhai_dynamic_to_json(&dynamic_array);
        assert_eq!(
            json_array,
            Value::Array(vec![Value::Number(serde_json::Number::from(1)), Value::Number(serde_json::Number::from(2))])
        );

        let mut dynamic_map = Map::new();
        dynamic_map.insert("key1".into(), Dynamic::from(1));
        dynamic_map.insert("key2".into(), Dynamic::from("value".to_string()));
        let dynamic_object = Dynamic::from_map(dynamic_map);
        let json_object = rhai_dynamic_to_json(&dynamic_object);
        let mut expected_map = serde_json::Map::new();
        expected_map.insert("key1".to_string(), Value::Number(serde_json::Number::from(1)));
        expected_map.insert("key2".to_string(), Value::String("value".to_string()));
        assert_eq!(json_object, Value::Object(expected_map));
    }
}
