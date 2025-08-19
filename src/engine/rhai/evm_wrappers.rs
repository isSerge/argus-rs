//! This module provides wrappers for EVM-related functionality in Rhai scripts.

use num_bigint::BigInt;
use rhai::Dynamic;
use rust_decimal::prelude::*;

fn decimals_constructor(value: Dynamic, decimals: i32) -> Result<BigInt, Box<rhai::EvalAltResult>> {
    if decimals > 0 {
        scale_by_decimals(value, decimals)
    } else {
        Err("Decimals must be positive".into())
    }
}

fn ether_constructor(value: Dynamic) -> Result<BigInt, Box<rhai::EvalAltResult>> {
    scale_by_decimals(value, 18)
}

fn gwei_constructor(value: Dynamic) -> Result<BigInt, Box<rhai::EvalAltResult>> {
    scale_by_decimals(value, 9)
}

// should be just name wrapper around BigInt
fn wei_constructor(value: Dynamic) -> BigInt {
    BigInt::from(value.as_int().unwrap_or(0))
}

fn usdc_constructor(value: Dynamic) -> Result<BigInt, Box<rhai::EvalAltResult>> {
    scale_by_decimals(value, 6)
}

fn usdt_constructor(value: Dynamic) -> Result<BigInt, Box<rhai::EvalAltResult>> {
    scale_by_decimals(value, 6)
}

fn wbtc_constructor(value: Dynamic) -> Result<BigInt, Box<rhai::EvalAltResult>> {
    scale_by_decimals(value, 8)
}

fn scale_by_decimals(value: Dynamic, decimals: i32) -> Result<BigInt, Box<rhai::EvalAltResult>> {
    let decimal = dynamic_to_decimal(value)?;
    let multiplier = Decimal::from_u64(10).unwrap().powi(decimals.into());
    let scaled = decimal * multiplier;
    let truncated = scaled.trunc();

    BigInt::from_str(truncated.to_string().as_str())
        .map_err(|_| "Failed to convert Decimal to BigInt".into())
}

fn dynamic_to_decimal(value: Dynamic) -> Result<Decimal, Box<rhai::EvalAltResult>> {
    if let Some(int_value) = value.as_int().ok() {
        return Ok(Decimal::from(int_value));
    } else if let Some(float_value) = value.as_float().ok() {
        return Ok(Decimal::from_f64(float_value).ok_or_else(|| "Invalid float value")?);
    } else if let Some(string_value) = value.into_string().ok() {
        return Ok(Decimal::from_str(&string_value).map_err(|_| "Invalid string value")?);
    } else {
        return Err("Unsupported type for Decimal conversion".into());
    }
}

/// Register EVM wrapper types and operations with Rhai engine
pub fn register_evm_wrappers_with_rhai(engine: &mut rhai::Engine) {
    // Register the Decimal type
    engine.register_type_with_name::<Decimal>("Decimal");

    // Register constructors
    engine.register_fn("decimal", decimals_constructor);
    engine.register_fn("ether", ether_constructor);
    engine.register_fn("gwei", gwei_constructor);
    engine.register_fn("wei", wei_constructor);
    engine.register_fn("usdc", usdc_constructor);
    engine.register_fn("usdt", usdt_constructor);
    engine.register_fn("wbtc", wbtc_constructor);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decimal_creation() {}
}
