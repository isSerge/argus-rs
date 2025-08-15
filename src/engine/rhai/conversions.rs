//! Data conversion utilities for Rhai scripting engine.
//!
//! This module handles conversion between blockchain data types and
//! Rhai-compatible types

use alloy::{
    consensus::TxType,
    dyn_abi::DynSolValue,
    primitives::{I256, U256},
};
use num_bigint::{BigInt, Sign};
use rhai::{Dynamic, Map};
use serde_json::Value;

use crate::{abi::DecodedLog, models::transaction::Transaction};

/// Converts a `DynSolValue` directly to a Rhai `Dynamic` value.
///
/// Large numbers (>i64/u64 range) are converted to `BigInt` to preserve
/// precision.
pub fn dyn_sol_value_to_rhai(value: &DynSolValue) -> Dynamic {
    match value {
        DynSolValue::Address(a) => a.to_checksum(None).into(),
        DynSolValue::Bool(b) => (*b).into(),
        DynSolValue::Bytes(b) => format!("0x{}", hex::encode(b)).into(),
        DynSolValue::FixedBytes(fb, _) => format!("0x{}", hex::encode(fb)).into(),

        // Handle large signed integers
        DynSolValue::Int(i, _) => {
            // Try to convert to i64 first, fallback to BigInt for large numbers
            if let Ok(small_int) = i64::try_from(*i) {
                small_int.into()
            } else {
                // Too large for i64, use BigInt representation
                i256_to_dynamic(*i)
            }
        }

        DynSolValue::String(s) => s.clone().into(),

        // Handle large unsigned integers
        DynSolValue::Uint(u, _) => {
            // Use the same logic as u256_to_dynamic
            u256_to_dynamic(*u)
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
/// Large numbers (>i64/u64 range) are converted to strings to preserve
/// precision.
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
            if *u <= U256::from(i64::MAX as u64) {
                Value::Number(serde_json::Number::from(u.to::<u64>() as i64))
            } else {
                Value::String(u.to_string())
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

/// Builds a Rhai `Map` from a transaction and optional receipt.
pub fn build_transaction_map(
    transaction: &Transaction,
    receipt: Option<&alloy::rpc::types::TransactionReceipt>,
) -> Map {
    let mut map = Map::new();

    if let Some(to) = transaction.to() {
        map.insert("to".into(), to.to_checksum(None).into());
    }

    map.insert("from".into(), transaction.from().to_checksum(None).into());
    map.insert("hash".into(), transaction.hash().to_string().into());

    // Convert transaction value directly to appropriate type
    let value_u256 = transaction.value();
    let value_dynamic = u256_to_dynamic(value_u256);
    map.insert("value".into(), value_dynamic);

    map.insert("gas_limit".into(), u256_to_dynamic(U256::from(transaction.gas())));
    map.insert("nonce".into(), u256_to_dynamic(U256::from(transaction.nonce())));
    map.insert("input".into(), format!("0x{}", hex::encode(transaction.input())).into());

    if let Some(block_number) = transaction.block_number() {
        map.insert("block_number".into(), u256_to_dynamic(U256::from(block_number)));
    } else {
        map.insert("block_number".into(), Dynamic::UNIT);
    }

    if let Some(transaction_index) = transaction.transaction_index() {
        map.insert("transaction_index".into(), u256_to_dynamic(U256::from(transaction_index)));
    } else {
        map.insert("transaction_index".into(), Dynamic::UNIT);
    }

    match transaction.transaction_type() {
        TxType::Legacy =>
            if let Some(gas_price) = transaction.gas_price() {
                map.insert("gas_price".into(), u256_to_dynamic(U256::from(gas_price)));
            } else {
                map.insert("gas_price".into(), Dynamic::UNIT);
            },
        TxType::Eip1559 => {
            map.insert(
                "max_fee_per_gas".into(),
                u256_to_dynamic(U256::from(transaction.max_fee_per_gas())),
            );
            if let Some(max_priority_fee_per_gas) = transaction.max_priority_fee_per_gas() {
                map.insert(
                    "max_priority_fee_per_gas".into(),
                    u256_to_dynamic(U256::from(max_priority_fee_per_gas)),
                );
            } else {
                map.insert("max_priority_fee_per_gas".into(), Dynamic::UNIT);
            }
        }
        _ => { /* Other transaction types are not explicitly handled for gas fields */ }
    }

    // Add receipt fields if available
    if let Some(receipt) = receipt {
        map.insert("gas_used".into(), u256_to_dynamic(U256::from(receipt.gas_used)));

        // Use the actual status from the receipt envelope
        map.insert("status".into(), receipt.inner.status().into());

        map.insert(
            "effective_gas_price".into(),
            u256_to_dynamic(U256::from(receipt.effective_gas_price)),
        );
    } else {
        // When no receipt is available, set receipt fields to UNIT (null)
        map.insert("gas_used".into(), Dynamic::UNIT);
        map.insert("status".into(), Dynamic::UNIT);
        map.insert("effective_gas_price".into(), Dynamic::UNIT);
    }

    map
}

/// Builds a Rhai `Map` from a decoded log.
pub fn build_log_map(log: &DecodedLog, params_map: Map) -> Map {
    let mut log_map = Map::new();
    log_map.insert("name".into(), log.name.clone().into());

    log_map.insert("params".into(), params_map.into());

    log_map.insert("address".into(), log.log.address().to_checksum(None).into());
    if let Some(block_number) = log.log.block_number() {
        log_map.insert("block_number".into(), u256_to_dynamic(U256::from(block_number)));
    } else {
        log_map.insert("block_number".into(), Dynamic::UNIT);
    }
    log_map.insert(
        "transaction_hash".into(),
        log.log.transaction_hash().unwrap_or_default().to_string().into(),
    );
    if let Some(log_index) = log.log.log_index() {
        log_map.insert("log_index".into(), u256_to_dynamic(U256::from(log_index)));
    } else {
        log_map.insert("log_index".into(), Dynamic::UNIT);
    }
    if let Some(transaction_index) = log.log.transaction_index() {
        log_map.insert("transaction_index".into(), u256_to_dynamic(U256::from(transaction_index)));
    } else {
        log_map.insert("transaction_index".into(), Dynamic::UNIT);
    }

    log_map
}

/// Converts a U256 value to the most appropriate Rhai type.
fn u256_to_dynamic(value: U256) -> Dynamic {
    // Try to fit in i64 (Rhai's native integer type)
    if value <= U256::from(i64::MAX as u64) {
        (value.to::<u64>() as i64).into()
    } else {
        // Too large for i64, use BigInt representation
        let (sign, bytes) = if value.is_zero() {
            (Sign::NoSign, vec![])
        } else {
            (Sign::Plus, value.to_be_bytes_vec())
        };
        Dynamic::from(BigInt::from_bytes_be(sign, &bytes))
    }
}

/// Converts an I256 value to the most appropriate Rhai type.
fn i256_to_dynamic(value: I256) -> Dynamic {
    // Try to fit in i64
    if let Ok(small_int) = i64::try_from(value) {
        small_int.into()
    } else {
        // Too large for i64, use BigInt representation
        let sign = if value.is_negative() { Sign::Minus } else { Sign::Plus };
        let bytes = value.abs().to_be_bytes::<32>();
        Dynamic::from(BigInt::from_bytes_be(sign, &bytes))
    }
}

/// Builds trigger data JSON from log parameters using the same conversion logic
/// as Rhai.
///
/// This ensures consistency between the data seen by Rhai scripts and the data
/// in trigger_data.
pub fn build_trigger_data_from_params(params: &[(String, DynSolValue)]) -> Value {
    let mut json_map = serde_json::Map::new();
    for (name, value) in params {
        json_map.insert(name.clone(), dyn_sol_value_to_json(value));
    }
    Value::Object(json_map)
}

#[cfg(test)]
mod tests {
    use alloy::{
        dyn_abi::Word,
        primitives::{Address, Function, I256, address, b256},
    };
    use num_bigint::BigInt;
    use serde_json::json;

    use super::*;
    use crate::test_helpers::{LogBuilder, ReceiptBuilder, TransactionBuilder};

    #[test]
    fn test_dyn_sol_value_to_rhai_basic_types() {
        // Address
        let addr = Address::repeat_byte(0x11);
        let result = dyn_sol_value_to_rhai(&DynSolValue::Address(addr));
        assert_eq!(result.cast::<String>(), "0x1111111111111111111111111111111111111111");

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
        // Large Uint (should become BigInt)
        let large_uint = U256::MAX;
        let result = dyn_sol_value_to_rhai(&DynSolValue::Uint(large_uint.into(), 256));
        let bigint_val = result.cast::<BigInt>();
        assert_eq!(bigint_val.to_string(), large_uint.to_string());

        // Uint at i64::MAX boundary (should stay as i64)
        let max_i64_as_u256 = U256::from(i64::MAX as u64);
        let result = dyn_sol_value_to_rhai(&DynSolValue::Uint(max_i64_as_u256.into(), 256));
        assert_eq!(result.cast::<i64>(), i64::MAX);

        // Uint just beyond i64::MAX (should become BigInt)
        let beyond_i64_max = U256::from(i64::MAX as u64) + U256::from(1);
        let result = dyn_sol_value_to_rhai(&DynSolValue::Uint(beyond_i64_max.into(), 256));
        let bigint_val = result.cast::<BigInt>();
        assert_eq!(bigint_val.to_string(), beyond_i64_max.to_string());
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
            ("value".to_string(), DynSolValue::Uint(U256::from(150).into(), 256)),
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
    fn test_u256_to_dynamic() {
        // Small value
        assert_eq!(u256_to_dynamic(U256::from(123)).cast::<i64>(), 123);

        // At boundary
        assert_eq!(u256_to_dynamic(U256::from(i64::MAX as u64)).cast::<i64>(), i64::MAX);

        // Beyond boundary
        let large = U256::from(i64::MAX as u64) + U256::from(1);
        let bigint_val = u256_to_dynamic(large).cast::<BigInt>();
        assert_eq!(bigint_val.to_string(), large.to_string());
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
            ("value".to_string(), DynSolValue::Uint(U256::from(150).into(), 256)),
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

    #[test]
    fn test_dyn_sol_value_to_rhai_bytes() {
        let bytes = vec![0x01, 0x02, 0x03];
        let result = dyn_sol_value_to_rhai(&DynSolValue::Bytes(bytes.clone().into()));
        assert_eq!(result.cast::<String>(), "0x010203");

        let mut fixed_bytes_array = [0u8; 32];
        fixed_bytes_array[0..3].copy_from_slice(&[0x04, 0x05, 0x06]);
        let fixed_bytes = Word::new(fixed_bytes_array);
        let result = dyn_sol_value_to_rhai(&DynSolValue::FixedBytes(fixed_bytes, 3));
        assert_eq!(
            result.cast::<String>(),
            "0x0405060000000000000000000000000000000000000000000000000000000000"
        );
    }

    #[test]
    fn test_dyn_sol_value_to_rhai_signed_integers() {
        // Small Int (should become i64)
        let result = dyn_sol_value_to_rhai(&DynSolValue::Int(
            alloy::primitives::I256::try_from(123i128).unwrap(),
            256,
        ));
        assert_eq!(result.cast::<i64>(), 123);

        let result = dyn_sol_value_to_rhai(&DynSolValue::Int(
            alloy::primitives::I256::try_from(-123i128).unwrap(),
            256,
        ));
        assert_eq!(result.cast::<i64>(), -123);

        // Int at i64::MAX boundary (should stay as i64)
        let max_i64_as_i256 = alloy::primitives::I256::try_from(i64::MAX as i128).unwrap();
        let result = dyn_sol_value_to_rhai(&DynSolValue::Int(max_i64_as_i256, 256));
        assert_eq!(result.cast::<i64>(), i64::MAX);

        // Int at i64::MIN boundary (should stay as i64)
        let min_i64_as_i256 = alloy::primitives::I256::try_from(i64::MIN as i128).unwrap();
        let result = dyn_sol_value_to_rhai(&DynSolValue::Int(min_i64_as_i256, 256));
        assert_eq!(result.cast::<i64>(), i64::MIN);

        // Int just beyond i64::MAX (should become BigInt)
        let beyond_i64_max = I256::try_from(i64::MAX as i128 + 1).unwrap();
        let result = dyn_sol_value_to_rhai(&DynSolValue::Int(beyond_i64_max, 256));
        let bigint_val = result.cast::<BigInt>();
        assert_eq!(bigint_val.to_string(), beyond_i64_max.to_string());

        // Int just beyond i64::MIN (should become BigInt)
        let beyond_i64_min = I256::try_from(i64::MIN as i128 - 1).unwrap();
        let result = dyn_sol_value_to_rhai(&DynSolValue::Int(beyond_i64_min, 256));
        let bigint_val = result.cast::<BigInt>();
        assert_eq!(bigint_val.to_string(), beyond_i64_min.to_string());
    }

    #[test]
    fn test_dyn_sol_value_to_rhai_tuple() {
        let tuple_values = vec![
            DynSolValue::Address(address!("1111111111111111111111111111111111111111")),
            DynSolValue::Uint(U256::from(42).into(), 256),
            DynSolValue::Bool(true),
            DynSolValue::String("tuple_item".to_string()),
        ];
        let result = dyn_sol_value_to_rhai(&DynSolValue::Tuple(tuple_values));
        let rhai_array = result.cast::<rhai::Array>();

        assert_eq!(rhai_array.len(), 4);
        assert_eq!(
            rhai_array[0].clone().cast::<String>(),
            "0x1111111111111111111111111111111111111111"
        );
        assert_eq!(rhai_array[1].clone().cast::<i64>(), 42);
        assert_eq!(rhai_array[2].clone().cast::<bool>(), true);
        assert_eq!(rhai_array[3].clone().cast::<String>(), "tuple_item");
    }

    #[test]
    fn test_dyn_sol_value_to_json_bytes() {
        let bytes = vec![0x01, 0x02, 0x03];
        let result = dyn_sol_value_to_json(&DynSolValue::Bytes(bytes.clone().into()));
        assert_eq!(result, json!("0x010203"));

        let mut fixed_bytes_array = [0u8; 32];
        fixed_bytes_array[0..3].copy_from_slice(&[0x04, 0x05, 0x06]);
        let fixed_bytes = Word::new(fixed_bytes_array);
        let result = dyn_sol_value_to_json(&DynSolValue::FixedBytes(fixed_bytes, 3));
        assert_eq!(
            result,
            json!("0x0405060000000000000000000000000000000000000000000000000000000000")
        );
    }

    #[test]
    fn test_dyn_sol_value_to_rhai_unhandled_variant() {
        let bytes = [0x01; 24];
        let function = Function::new(bytes);
        let result = dyn_sol_value_to_rhai(&DynSolValue::Function(function));
        assert!(result.is_unit());
    }

    #[test]
    fn test_dyn_sol_value_to_json_signed_integers() {
        // Small Int (should become number)
        let result = dyn_sol_value_to_json(&DynSolValue::Int(
            alloy::primitives::I256::try_from(123i128).unwrap(),
            256,
        ));
        assert_eq!(result, json!(123));

        let result = dyn_sol_value_to_json(&DynSolValue::Int(
            alloy::primitives::I256::try_from(-123i128).unwrap(),
            256,
        ));
        assert_eq!(result, json!(-123));

        // Int at i64::MAX boundary (should stay as number)
        let max_i64_as_i256 = alloy::primitives::I256::try_from(i64::MAX as i128).unwrap();
        let result = dyn_sol_value_to_json(&DynSolValue::Int(max_i64_as_i256, 256));
        assert_eq!(result, json!(i64::MAX));

        // Int at i64::MIN boundary (should stay as number)
        let min_i64_as_i256 = alloy::primitives::I256::try_from(i64::MIN as i128).unwrap();
        let result = dyn_sol_value_to_json(&DynSolValue::Int(min_i64_as_i256, 256));
        assert_eq!(result, json!(i64::MIN));

        // Int just beyond i64::MAX (should become string)
        let beyond_i64_max = alloy::primitives::I256::try_from(i64::MAX as i128 + 1).unwrap();
        let result = dyn_sol_value_to_json(&DynSolValue::Int(beyond_i64_max, 256));
        assert_eq!(result, json!(beyond_i64_max.to_string()));

        // Int just beyond i64::MIN (should become string)
        let beyond_i64_min = alloy::primitives::I256::try_from(i64::MIN as i128 - 1).unwrap();
        let result = dyn_sol_value_to_json(&DynSolValue::Int(beyond_i64_min, 256));
        assert_eq!(result, json!(beyond_i64_min.to_string()));
    }

    #[test]
    fn test_dyn_sol_value_to_json_tuple() {
        let tuple_values = vec![
            DynSolValue::Address(address!("1111111111111111111111111111111111111111")),
            DynSolValue::Uint(U256::from(42).into(), 256),
            DynSolValue::Bool(true),
            DynSolValue::String("tuple_item".to_string()),
        ];
        let result = dyn_sol_value_to_json(&DynSolValue::Tuple(tuple_values));
        assert_eq!(
            result,
            json!(["0x1111111111111111111111111111111111111111", 42, true, "tuple_item"])
        );
    }

    #[test]
    fn test_dyn_sol_value_to_json_unhandled_variant() {
        // Use a DynSolValue variant that is not explicitly handled
        let bytes = [0x01; 24];
        let function = Function::new(bytes);
        let result = dyn_sol_value_to_json(&DynSolValue::Function(function));
        assert_eq!(result, json!(null));
    }

    #[test]
    fn test_build_transaction_map_contract_creation() {
        let tx = TransactionBuilder::new().to(None).build();
        let map = build_transaction_map(&tx, None);

        assert!(!map.contains_key("to"));
        assert_eq!(map.get("from").unwrap().clone().cast::<String>(), tx.from().to_checksum(None));
        assert_eq!(map.get("hash").unwrap().clone().cast::<String>(), tx.hash().to_string());
        assert_eq!(map.get("value").unwrap().clone().cast::<i64>(), tx.value().to::<i64>());

        // Verify receipt fields are UNIT when no receipt provided
        assert!(map.get("gas_used").unwrap().is_unit());
        assert!(map.get("status").unwrap().is_unit());
        assert!(map.get("effective_gas_price").unwrap().is_unit());
    }

    #[test]
    fn test_build_transaction_map_all_fields() {
        let tx = TransactionBuilder::new()
            .to(Some(address!("0x0000000000000000000000000000000000000003")))
            .value(U256::from(1000))
            .gas_limit(21000)
            .nonce(5)
            .block_number(12345)
            .transaction_index(7)
            .input(vec![0x11, 0x22, 0x33].into())
            .max_fee_per_gas(U256::from(200))
            .max_priority_fee_per_gas(U256::from(100))
            .tx_type(TxType::Eip1559)
            .build();

        let map = build_transaction_map(&tx, None);

        assert_eq!(
            map.get("to").unwrap().clone().cast::<String>(),
            tx.to().unwrap().to_checksum(None)
        );
        assert_eq!(map.get("from").unwrap().clone().cast::<String>(), tx.from().to_checksum(None));
        assert_eq!(map.get("hash").unwrap().clone().cast::<String>(), tx.hash().to_string());
        assert_eq!(map.get("value").unwrap().clone().cast::<i64>(), tx.value().to::<i64>());
        assert_eq!(map.get("gas_limit").unwrap().clone().cast::<i64>(), tx.gas() as i64);
        assert_eq!(map.get("nonce").unwrap().clone().cast::<i64>(), tx.nonce() as i64);
        assert_eq!(
            map.get("block_number").unwrap().clone().cast::<i64>(),
            tx.block_number().unwrap() as i64
        );
        assert_eq!(
            map.get("transaction_index").unwrap().clone().cast::<i64>(),
            tx.transaction_index().unwrap() as i64
        );
        assert_eq!(
            map.get("input").unwrap().clone().cast::<String>(),
            format!("0x{}", hex::encode(tx.input()))
        );

        // EIP-1559 specific fields
        assert_eq!(
            map.get("max_fee_per_gas").unwrap().clone().cast::<i64>(),
            tx.max_fee_per_gas() as i64
        );
        assert_eq!(
            map.get("max_priority_fee_per_gas").unwrap().clone().cast::<i64>(),
            tx.max_priority_fee_per_gas().unwrap() as i64
        );
        assert!(!map.contains_key("gas_price"));
    }

    #[test]
    fn test_build_transaction_map_legacy_fields() {
        let tx = TransactionBuilder::new()
            .to(Some(address!("0x0000000000000000000000000000000000000003")))
            .value(U256::from(1000))
            .gas_limit(21000)
            .nonce(5)
            .block_number(12345)
            .transaction_index(7)
            .input(vec![0x11, 0x22, 0x33].into())
            .gas_price(U256::from(150))
            .tx_type(TxType::Legacy)
            .build();

        let map = build_transaction_map(&tx, None);

        assert_eq!(
            map.get("to").unwrap().clone().cast::<String>(),
            tx.to().unwrap().to_checksum(None)
        );
        assert_eq!(map.get("from").unwrap().clone().cast::<String>(), tx.from().to_checksum(None));
        assert_eq!(map.get("hash").unwrap().clone().cast::<String>(), tx.hash().to_string());
        assert_eq!(map.get("value").unwrap().clone().cast::<i64>(), tx.value().to::<i64>());
        assert_eq!(map.get("gas_limit").unwrap().clone().cast::<i64>(), tx.gas() as i64);
        assert_eq!(map.get("nonce").unwrap().clone().cast::<i64>(), tx.nonce() as i64);
        assert_eq!(
            map.get("block_number").unwrap().clone().cast::<i64>(),
            tx.block_number().unwrap() as i64
        );
        assert_eq!(
            map.get("transaction_index").unwrap().clone().cast::<i64>(),
            tx.transaction_index().unwrap() as i64
        );
        assert_eq!(
            map.get("input").unwrap().clone().cast::<String>(),
            format!("0x{}", hex::encode(tx.input()))
        );

        // Legacy specific fields
        assert_eq!(
            map.get("gas_price").unwrap().clone().cast::<i64>(),
            tx.gas_price().unwrap() as i64
        );
        assert!(!map.contains_key("max_fee_per_gas"));
        assert!(!map.contains_key("max_priority_fee_per_gas"));
    }

    #[test]
    fn test_build_log_map_all_fields() {
        let log_address = address!("0x0000000000000000000000000000000000000001");
        let tx_hash = b256!("0x1111111111111111111111111111111111111111111111111111111111111111");

        let log_raw = LogBuilder::new()
            .address(log_address)
            .block_number(100)
            .transaction_hash(tx_hash)
            .log_index(5)
            .transaction_index(2)
            .build();

        let decoded_log =
            DecodedLog { name: "Transfer".to_string(), params: vec![], log: log_raw.into() };

        let params_map = Map::new(); // Empty for this test, as we're testing log fields
        let map = build_log_map(&decoded_log, params_map);

        assert_eq!(map.get("name").unwrap().clone().cast::<String>(), "Transfer");
        assert_eq!(
            map.get("address").unwrap().clone().cast::<String>(),
            log_address.to_checksum(None)
        );
        assert_eq!(map.get("block_number").unwrap().clone().cast::<i64>(), 100);
        assert_eq!(
            map.get("transaction_hash").unwrap().clone().cast::<String>(),
            tx_hash.to_string()
        );
        assert_eq!(map.get("log_index").unwrap().clone().cast::<i64>(), 5);
        assert_eq!(map.get("transaction_index").unwrap().clone().cast::<i64>(), 2);
    }

    #[test]
    fn test_build_transaction_map_with_receipt() {
        let tx_hash = b256!("0x1111111111111111111111111111111111111111111111111111111111111111");
        let tx = TransactionBuilder::new()
            .hash(tx_hash)
            .to(Some(address!("0x0000000000000000000000000000000000000003")))
            .value(U256::from(1000))
            .gas_limit(21000)
            .nonce(5)
            .build();

        let receipt = ReceiptBuilder::new()
            .transaction_hash(tx_hash)
            .gas_used(18500)
            .status(true)
            .effective_gas_price(145)
            .build();

        let map = build_transaction_map(&tx, Some(&receipt));

        // Verify basic transaction fields are still there
        assert_eq!(
            map.get("to").unwrap().clone().cast::<String>(),
            tx.to().unwrap().to_checksum(None)
        );
        assert_eq!(map.get("hash").unwrap().clone().cast::<String>(), tx.hash().to_string());

        // Verify receipt fields are included
        assert_eq!(map.get("gas_used").unwrap().clone().cast::<i64>(), 18500);
        assert_eq!(map.get("status").unwrap().clone().cast::<bool>(), true);
        assert_eq!(map.get("effective_gas_price").unwrap().clone().cast::<i64>(), 145);
    }

    #[test]
    fn test_build_transaction_map_with_failed_receipt() {
        let tx_hash = b256!("0x2222222222222222222222222222222222222222222222222222222222222222");
        let tx = TransactionBuilder::new()
            .hash(tx_hash)
            .to(Some(address!("0x0000000000000000000000000000000000000004")))
            .build();

        let receipt = ReceiptBuilder::new()
            .transaction_hash(tx_hash)
            .gas_used(21000)
            .status(false) // Failure
            .build();

        let map = build_transaction_map(&tx, Some(&receipt));

        // Verify receipt fields for failed transaction
        assert_eq!(map.get("gas_used").unwrap().clone().cast::<i64>(), 21000);
        assert_eq!(map.get("status").unwrap().clone().cast::<bool>(), false);
    }
}
