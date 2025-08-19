//! This module provides wrappers for EVM-related functionality in Rhai scripts.
//! It includes constructors for various token types and utility functions for
//! scaling values.

use num_bigint::BigInt;
use rhai::Dynamic;
use rust_decimal::prelude::*;

/// Scale a value by a given number of decimal places and return a `BigInt`.
fn decimals(value: Dynamic, decimals: i64) -> Result<BigInt, Box<rhai::EvalAltResult>> {
    if decimals >= 0 {
        scale_by_decimals(value, decimals as u32)
    } else {
        Err("Decimals must be non-negative".into())
    }
}

/// Constructor for ether value (18 decimals)
fn ether(value: Dynamic) -> Result<BigInt, Box<rhai::EvalAltResult>> {
    scale_by_decimals(value, 18)
}

/// Constructor for gwei value (9 decimals)
fn gwei(value: Dynamic) -> Result<BigInt, Box<rhai::EvalAltResult>> {
    scale_by_decimals(value, 9)
}

/// Constructor for wei value (0 decimals)
fn wei(value: Dynamic) -> Result<BigInt, Box<rhai::EvalAltResult>> {
    scale_by_decimals(value, 0)
}

/// Constructor for USDC value (6 decimals)
fn usdc(value: Dynamic) -> Result<BigInt, Box<rhai::EvalAltResult>> {
    scale_by_decimals(value, 6)
}

/// Constructor for USDT value (6 decimals)
fn usdt(value: Dynamic) -> Result<BigInt, Box<rhai::EvalAltResult>> {
    usdc(value) // Also 6 decimals, same as USDC
}

/// Constructor for WBTC value (8 decimals)
fn wbtc(value: Dynamic) -> Result<BigInt, Box<rhai::EvalAltResult>> {
    scale_by_decimals(value, 8)
}

/// Scale a Rhai Dynamic value by a given number of decimal places and return a
/// `BigInt`.
fn scale_by_decimals(value: Dynamic, decimals: u32) -> Result<BigInt, Box<rhai::EvalAltResult>> {
    let decimal = dynamic_to_decimal(value)?;
    let multiplier = Decimal::from_u64(10).unwrap().powi(decimals.into());
    let scaled = decimal * multiplier;
    let truncated = scaled.trunc();

    BigInt::from_str(truncated.to_string().as_str())
        .map_err(|_| "Failed to convert Decimal to BigInt".into())
}

/// Convert a Rhai Dynamic value to a Decimal.
fn dynamic_to_decimal(value: Dynamic) -> Result<Decimal, Box<rhai::EvalAltResult>> {
    let value_type = value.type_name();

    if value.is::<Decimal>() {
        return Ok(value.clone_cast::<Decimal>());
    }
    if let Some(int_value) = value.as_int().ok() {
        return Ok(Decimal::from(int_value));
    }
    if let Some(float_value) = value.as_float().ok() {
        return Decimal::from_f64(float_value).ok_or_else(|| "Invalid float value".into());
    }
    // Handle BigInt by converting it to a string for parsing.
    if value.is::<BigInt>() {
        let bigint_value = value.clone_cast::<BigInt>();
        return Decimal::from_str(&bigint_value.to_string())
            .map_err(|_| "Invalid BigInt string".into());
    }
    // Handle string values as a fallback.
    if let Some(string_value) = value.into_string().ok() {
        return Decimal::from_str(&string_value).map_err(|_| "Invalid number string".into());
    }

    Err(format!("Cannot convert value of type {:?} to Decimal", value_type).into())
}

/// Register EVM wrapper types and operations with Rhai engine
pub fn register_evm_wrappers_with_rhai(engine: &mut rhai::Engine) {
    // Register the Decimal type
    engine.register_type_with_name::<Decimal>("Decimal");

    // Register constructors
    engine.register_fn("decimals", decimals);
    engine.register_fn("ether", ether);
    engine.register_fn("gwei", gwei);
    engine.register_fn("wei", wei);
    engine.register_fn("usdc", usdc);
    engine.register_fn("usdt", usdt);
    engine.register_fn("wbtc", wbtc);
}

#[cfg(test)]
mod tests {
    use alloy::primitives::U256;

    use super::*;
    use crate::engine::rhai::conversions::u256_to_bigint_dynamic;

    #[test]
    fn test_dynamic_to_decimal() {
        let dynamic_int = Dynamic::from(42);
        let dynamic_float = Dynamic::from(3.14);
        let dynamic_bigint_string = u256_to_bigint_dynamic(U256::MAX);
        let dynamic_wrong_type = Dynamic::from("not a number");

        let decimal_int = dynamic_to_decimal(dynamic_int).unwrap();
        let decimal_float = dynamic_to_decimal(dynamic_float).unwrap();
        let decimal_bigint = dynamic_to_decimal(dynamic_bigint_string).unwrap();
        let decimal_wrong_type = dynamic_to_decimal(dynamic_wrong_type);

        assert_eq!(decimal_int, Decimal::from(42));
        assert_eq!(decimal_float, Decimal::from_f64(3.14).unwrap());
        assert_eq!(decimal_bigint, Decimal::from_str(&U256::MAX.to_string()).unwrap());
        assert!(decimal_wrong_type.is_err());
    }

    #[test]
    fn test_scale_by_decimals() {
        let dynamic_value = Dynamic::from(42);
        let scaled_value = scale_by_decimals(dynamic_value, 6).unwrap();
        assert_eq!(scaled_value, BigInt::from(42000000));

        let dynamic_value_float = Dynamic::from(3.14);
        let scaled_value_float = scale_by_decimals(dynamic_value_float, 6).unwrap();
        assert_eq!(scaled_value_float, BigInt::from(3140000));

        let dynamic_value_bigint = u256_to_bigint_dynamic(U256::from(1000));
        let scaled_value_bigint = scale_by_decimals(dynamic_value_bigint, 3).unwrap();
        assert_eq!(scaled_value_bigint, BigInt::from(1000000)); 
    }
}
