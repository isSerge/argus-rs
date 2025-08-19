//! Data conversion utilities for Rhai scripting engine.
//!
//! This module handles conversion between blockchain data types and
//! Rhai-compatible types

use alloy::{
    consensus::TxType,
    dyn_abi::DynSolValue,
    primitives::{I256, U256},
    rpc::types::TransactionReceipt,
};
use num_bigint::{BigInt, Sign};
use rhai::{Dynamic, Map};
use serde_json::{Value, json};

use crate::{abi::DecodedLog, models::transaction::Transaction};

/// Converts a `DynSolValue` directly to a Rhai `Dynamic` value.
///
/// `uint256` and `int256` types are always converted to `BigInt` to ensure
/// type consistency within Rhai scripts, preventing comparison errors.
pub fn dyn_sol_value_to_rhai(value: &DynSolValue) -> Dynamic {
    match value {
        DynSolValue::Address(a) => a.to_checksum(None).into(),
        DynSolValue::Bool(b) => (*b).into(),
        DynSolValue::Bytes(b) => format!("0x{}", hex::encode(b)).into(),
        DynSolValue::FixedBytes(fb, _) => format!("0x{}", hex::encode(fb)).into(),

        // Always convert large integer types from ABI to BigInt for consistency.
        DynSolValue::Int(i, _) => i256_to_bigint_dynamic(*i),
        DynSolValue::Uint(u, _) => u256_to_bigint_dynamic(*u),

        DynSolValue::String(s) => s.clone().into(),

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

    // --- Selective Conversion ---
    // Values that can exceed i64::MAX are always converted to BigInt.
    map.insert("value".into(), u256_to_bigint_dynamic(transaction.value()));

    // Values that are bounded (typically u64) are converted to i64.
    map.insert("gas_limit".into(), (transaction.gas() as i64).into());
    map.insert("nonce".into(), (transaction.nonce() as i64).into());
    map.insert("input".into(), format!("0x{}", hex::encode(transaction.input())).into());

    if let Some(block_number) = transaction.block_number() {
        map.insert("block_number".into(), (block_number as i64).into());
    } else {
        map.insert("block_number".into(), Dynamic::UNIT);
    }

    if let Some(transaction_index) = transaction.transaction_index() {
        map.insert("transaction_index".into(), (transaction_index as i64).into());
    } else {
        map.insert("transaction_index".into(), Dynamic::UNIT);
    }

    match transaction.transaction_type() {
        TxType::Legacy =>
            if let Some(gas_price) = transaction.gas_price() {
                map.insert("gas_price".into(), u256_to_bigint_dynamic(U256::from(gas_price)));
            } else {
                map.insert("gas_price".into(), Dynamic::UNIT);
            },
        TxType::Eip1559 => {
            map.insert(
                "max_fee_per_gas".into(),
                u256_to_bigint_dynamic(U256::from(transaction.max_fee_per_gas())),
            );
            if let Some(max_priority_fee_per_gas) = transaction.max_priority_fee_per_gas() {
                map.insert(
                    "max_priority_fee_per_gas".into(),
                    u256_to_bigint_dynamic(U256::from(max_priority_fee_per_gas)),
                );
            } else {
                map.insert("max_priority_fee_per_gas".into(), Dynamic::UNIT);
            }
        }
        _ => { /* Other transaction types are not explicitly handled for gas fields */ }
    }

    // Add receipt fields if available
    if let Some(receipt) = receipt {
        map.insert("gas_used".into(), (receipt.gas_used as i64).into());
        // status is 0 or 1, fits in i64.
        map.insert("status".into(), (receipt.inner.status() as i64).into());
        // effective_gas_price can be large.
        map.insert(
            "effective_gas_price".into(),
            u256_to_bigint_dynamic(U256::from(receipt.effective_gas_price)),
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

    // Bounded u64 values become i64
    if let Some(block_number) = log.log.block_number() {
        log_map.insert("block_number".into(), (block_number as i64).into());
    } else {
        log_map.insert("block_number".into(), Dynamic::UNIT);
    }
    log_map.insert(
        "transaction_hash".into(),
        log.log.transaction_hash().unwrap_or_default().to_string().into(),
    );
    if let Some(log_index) = log.log.log_index() {
        log_map.insert("log_index".into(), (log_index as i64).into());
    } else {
        log_map.insert("log_index".into(), Dynamic::UNIT);
    }
    if let Some(transaction_index) = log.log.transaction_index() {
        log_map.insert("transaction_index".into(), (transaction_index as i64).into());
    } else {
        log_map.insert("transaction_index".into(), Dynamic::UNIT);
    }

    log_map
}

/// Converts a U256 value unconditionally to a Rhai `BigInt` dynamic type.
fn u256_to_bigint_dynamic(value: U256) -> Dynamic {
    let (sign, bytes) = if value.is_zero() {
        (Sign::NoSign, vec![])
    } else {
        (Sign::Plus, value.to_be_bytes_vec())
    };
    Dynamic::from(BigInt::from_bytes_be(sign, &bytes))
}

/// Converts an I256 value unconditionally to a Rhai `BigInt` dynamic type.
fn i256_to_bigint_dynamic(value: I256) -> Dynamic {
    let (sign, abs_value) = value.into_sign_and_abs();
    let rhai_sign = match sign {
        alloy::primitives::Sign::Positive => Sign::Plus,
        alloy::primitives::Sign::Negative => Sign::Minus,
    };
    let bytes = abs_value.to_be_bytes_vec();
    Dynamic::from(BigInt::from_bytes_be(rhai_sign, &bytes))
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

/// Builds trigger data JSON from a transaction, ensuring consistency with Rhai
/// script data.
pub fn build_trigger_data_from_transaction(
    transaction: &Transaction,
    receipt: Option<&TransactionReceipt>,
) -> Value {
    let mut map = serde_json::Map::new();

    if let Some(to) = transaction.to() {
        map.insert("to".to_string(), json!(to.to_checksum(None)));
    }
    map.insert("from".to_string(), json!(transaction.from().to_checksum(None)));
    map.insert("hash".to_string(), json!(transaction.hash().to_string()));
    map.insert("value".to_string(), json!(transaction.value().to_string()));
    map.insert("gas_limit".to_string(), json!(transaction.gas()));
    map.insert("nonce".to_string(), json!(transaction.nonce()));
    map.insert("input".to_string(), json!(format!("0x{}", hex::encode(transaction.input()))));

    if let Some(block_number) = transaction.block_number() {
        map.insert("block_number".to_string(), json!(block_number));
    }
    if let Some(transaction_index) = transaction.transaction_index() {
        map.insert("transaction_index".to_string(), json!(transaction_index));
    }

    match transaction.transaction_type() {
        TxType::Legacy =>
            if let Some(gas_price) = transaction.gas_price() {
                map.insert("gas_price".to_string(), json!(gas_price));
            },
        TxType::Eip1559 => {
            map.insert("max_fee_per_gas".to_string(), json!(transaction.max_fee_per_gas()));
            if let Some(max_priority_fee_per_gas) = transaction.max_priority_fee_per_gas() {
                map.insert("max_priority_fee_per_gas".to_string(), json!(max_priority_fee_per_gas));
            }
        }
        _ => {}
    }

    if let Some(receipt) = receipt {
        map.insert("gas_used".to_string(), json!(receipt.gas_used));
        map.insert("status".to_string(), json!(receipt.inner.status() as u64));
        map.insert("effective_gas_price".to_string(), json!(receipt.effective_gas_price));
    }

    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use alloy::primitives::{Address, I256, U256, address, b256};
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

        // Small Uint (should now become BigInt for consistency)
        let result = dyn_sol_value_to_rhai(&DynSolValue::Uint(U256::from(123), 256));
        assert_eq!(result.cast::<BigInt>(), BigInt::from(123));
    }

    #[test]
    fn test_dyn_sol_value_to_rhai_large_numbers() {
        // Large Uint (should become BigInt)
        let large_uint = U256::MAX;
        let result = dyn_sol_value_to_rhai(&DynSolValue::Uint(large_uint, 256));
        let bigint_val = result.cast::<BigInt>();
        assert_eq!(bigint_val.to_string(), large_uint.to_string());
    }

    #[test]
    fn test_dyn_sol_value_to_rhai_arrays() {
        let arr = vec![
            DynSolValue::Uint(U256::from(1), 256),
            DynSolValue::Bool(false),
            DynSolValue::String("test".to_string()),
        ];

        let result = dyn_sol_value_to_rhai(&DynSolValue::Array(arr));
        let rhai_array = result.cast::<rhai::Array>();

        assert_eq!(rhai_array.len(), 3);
        assert_eq!(rhai_array[0].clone().cast::<BigInt>(), BigInt::from(1));
        assert_eq!(rhai_array[1].clone().cast::<bool>(), false);
        assert_eq!(rhai_array[2].clone().cast::<String>(), "test");
    }

    #[test]
    fn test_build_log_params_map() {
        let params = vec![
            ("value".to_string(), DynSolValue::Uint(U256::from(150), 256)),
            (
                "sender".to_string(),
                DynSolValue::Address(address!("1111111111111111111111111111111111111111")),
            ),
        ];

        let result = build_log_params_map(&params);

        assert_eq!(result.len(), 2);
        assert_eq!(result.get("value").unwrap().clone().cast::<BigInt>(), BigInt::from(150));
        assert_eq!(
            result.get("sender").unwrap().clone().cast::<String>(),
            "0x1111111111111111111111111111111111111111"
        );
    }

    #[test]
    fn test_u256_to_bigint_dynamic() {
        // Small value
        assert_eq!(u256_to_bigint_dynamic(U256::from(123)).cast::<BigInt>(), BigInt::from(123));

        // At boundary
        assert_eq!(
            u256_to_bigint_dynamic(U256::from(i64::MAX as u64)).cast::<BigInt>(),
            BigInt::from(i64::MAX)
        );

        // Beyond boundary
        let large = U256::from(i64::MAX as u64) + U256::from(1);
        let bigint_val = u256_to_bigint_dynamic(large).cast::<BigInt>();
        assert_eq!(bigint_val.to_string(), large.to_string());
    }

    #[test]
    fn test_dyn_sol_value_to_json_large_numbers() {
        // Large Uint (should become string)
        let large_uint = U256::MAX;
        let result = dyn_sol_value_to_json(&DynSolValue::Uint(large_uint, 256));
        assert_eq!(result, json!(large_uint.to_string()));

        // Uint at i64::MAX boundary (should stay as number)
        let max_i64_as_u256 = U256::from(i64::MAX as u64);
        let result = dyn_sol_value_to_json(&DynSolValue::Uint(max_i64_as_u256, 256));
        assert_eq!(result, json!(i64::MAX));

        // Uint just beyond i64::MAX (should become string)
        let beyond_i64_max = U256::from(i64::MAX as u64) + U256::from(1);
        let result = dyn_sol_value_to_json(&DynSolValue::Uint(beyond_i64_max, 256));
        assert_eq!(result, json!(beyond_i64_max.to_string()));
    }

    #[test]
    fn test_dyn_sol_value_to_rhai_signed_integers() {
        // Small Int (should become BigInt)
        let result = dyn_sol_value_to_rhai(&DynSolValue::Int(I256::try_from(123).unwrap(), 256));
        assert_eq!(result.cast::<BigInt>(), BigInt::from(123));

        let result = dyn_sol_value_to_rhai(&DynSolValue::Int(I256::try_from(-123).unwrap(), 256));
        assert_eq!(result.cast::<BigInt>(), BigInt::from(-123));

        // Int at i64::MAX boundary (should become BigInt)
        let result =
            dyn_sol_value_to_rhai(&DynSolValue::Int(I256::try_from(i64::MAX).unwrap(), 256));
        assert_eq!(result.cast::<BigInt>(), BigInt::from(i64::MAX));

        // Int at i64::MIN boundary (should become BigInt)
        let result =
            dyn_sol_value_to_rhai(&DynSolValue::Int(I256::try_from(i64::MIN).unwrap(), 256));
        assert_eq!(result.cast::<BigInt>(), BigInt::from(i64::MIN));

        // Int just beyond i64::MAX (should become BigInt)
        let beyond_i64_max = I256::try_from(i64::MAX as i128 + 1).unwrap();
        let result = dyn_sol_value_to_rhai(&DynSolValue::Int(beyond_i64_max, 256));
        let bigint_val = result.cast::<BigInt>();
        assert_eq!(bigint_val.to_string(), beyond_i64_max.to_string());
    }

    /// Confirms that fields that can be large are always BigInt, and bounded
    /// fields are i64.
    #[test]
    fn test_build_transaction_map_hybrid_types_comprehensive() {
        let tx = TransactionBuilder::new()
            .value(U256::from(1000)) // Small value that could have been i64
            .gas_limit(21000)
            .nonce(5)
            .block_number(12345)
            .max_fee_per_gas(U256::from(200)) // Small value
            .tx_type(TxType::Eip1559)
            .build();

        let receipt = ReceiptBuilder::new()
            .gas_used(18500)
            .status(true)
            .effective_gas_price(150) // Small value
            .build();

        let map = build_transaction_map(&tx, Some(&receipt));

        // --- Assert BigInt types for potentially large values ---
        let value = map.get("value").unwrap().clone();
        assert_eq!(value.type_name(), "num_bigint::bigint::BigInt");
        assert_eq!(value.cast::<BigInt>(), BigInt::from(1000u64));

        let max_fee = map.get("max_fee_per_gas").unwrap().clone();
        assert_eq!(max_fee.type_name(), "num_bigint::bigint::BigInt");
        assert_eq!(max_fee.cast::<BigInt>(), BigInt::from(200u64));

        let effective_price = map.get("effective_gas_price").unwrap().clone();
        assert_eq!(effective_price.type_name(), "num_bigint::bigint::BigInt");
        assert_eq!(effective_price.cast::<BigInt>(), BigInt::from(150u64));

        // --- Assert i64 types for bounded values ---
        let gas_limit = map.get("gas_limit").unwrap().clone();
        assert_eq!(gas_limit.type_name(), "i64");
        assert_eq!(gas_limit.cast::<i64>(), 21000);

        let nonce = map.get("nonce").unwrap().clone();
        assert_eq!(nonce.type_name(), "i64");
        assert_eq!(nonce.cast::<i64>(), 5);

        let block_num = map.get("block_number").unwrap().clone();
        assert_eq!(block_num.type_name(), "i64");
        assert_eq!(block_num.cast::<i64>(), 12345);

        let gas_used = map.get("gas_used").unwrap().clone();
        assert_eq!(gas_used.type_name(), "i64");
        assert_eq!(gas_used.cast::<i64>(), 18500);

        let status = map.get("status").unwrap().clone();
        assert_eq!(status.type_name(), "i64");
        assert_eq!(status.cast::<i64>(), 1);
    }

    #[test]
    fn test_build_log_map_all_fields() {
        let log_address = address!("0000000000000000000000000000000000000001");
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

        let params_map = Map::new(); // Empty for this test
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
        let tx = TransactionBuilder::new().hash(tx_hash).build();

        let receipt = ReceiptBuilder::new()
            .transaction_hash(tx_hash)
            .gas_used(18500)
            .status(true)
            .effective_gas_price(145)
            .build();

        let map = build_transaction_map(&tx, Some(&receipt));

        // Verify receipt fields are included with correct types
        assert_eq!(map.get("gas_used").unwrap().clone().cast::<i64>(), 18500);
        assert_eq!(map.get("status").unwrap().clone().cast::<i64>(), 1);
        assert_eq!(
            map.get("effective_gas_price").unwrap().clone().cast::<BigInt>(),
            BigInt::from(145u64)
        );
    }
}
