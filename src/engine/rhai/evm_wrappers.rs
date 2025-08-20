//! # Rhai EVM Wrappers
//!
//! This module provides essential wrappers for EVM-related functionalities
//! to be used within Rhai scripts. It facilitates the handling of different
//! token denominations by offering convenient constructors for common token
//! types and utility functions for value scaling.
//!
//! ## Features:
//!
//! - **Token Constructors**: Functions like `ether`, `gwei`, `usdc`, etc.,
//!   allow for easy conversion of numeric values into the appropriate `BigInt`
//!   representation, scaled to the correct number of decimal places.
//! - **Decimal Scaling**: A generic `decimals` function is available for tokens
//!   with a variable number of decimal places.
//!
//! ## Usage Example:
//!
//! ```rhai
//! // Example of using the wrappers in a Rhai script
//! let price = ether(1.5); // Represents 1.5 ETH in wei
//! let fee = gwei(100);   // Represents 100 Gwei in wei
//!
//! // Custom token with 12 decimals
//! let custom_token_amount = decimals(50.25, 12);
//! ```

use num_bigint::BigInt;
use rhai::Dynamic;
use rust_decimal::prelude::*;

/// Scales a `Dynamic` value by a specified number of decimal places.
///
/// This function is used to convert a human-readable number (like `1.23`)
/// into its integer representation based on the token's decimals. For example,
/// a token with 2 decimal places would convert `1.23` to `123`.
///
/// # Arguments
///
/// * `value` - The `Dynamic` value to scale. Can be an integer, float, or
///   string.
/// * `decimals` - The number of decimal places to scale by. Must be
///   non-negative.
///
/// # Example
///
/// ```rhai
/// let amount = decimals(1.23, 2); // returns 123
/// ```
fn decimals(value: Dynamic, decimals: i64) -> Result<BigInt, Box<rhai::EvalAltResult>> {
    if decimals >= 0 {
        scale_by_decimals(value, decimals as u32)
    } else {
        Err("Decimals must be non-negative".into())
    }
}

/// Converts a value to wei, assuming it is denominated in ether (18 decimals).
///
/// # Arguments
///
/// * `value` - The value in ether to convert.
///
/// # Example
///
/// ```rhai
/// let one_ether_in_wei = ether(1); // returns 1000000000000000000
/// ```
fn ether(value: Dynamic) -> Result<BigInt, Box<rhai::EvalAltResult>> {
    scale_by_decimals(value, 18)
}

/// Converts a value to wei, assuming it is denominated in gwei (9 decimals).
///
/// # Arguments
///
/// * `value` - The value in gwei to convert.
///
/// # Example
///
/// ```rhai
/// let one_gwei_in_wei = gwei(1); // returns 1000000000
/// ```
fn gwei(value: Dynamic) -> Result<BigInt, Box<rhai::EvalAltResult>> {
    scale_by_decimals(value, 9)
}

/// Converts a value to wei (0 decimals).
///
/// Since wei is the base unit, this function effectively truncates any
/// fractional part of the input value.
///
/// # Arguments
///
/// * `value` - The value in wei to convert.
///
/// # Example
///
/// ```rhai
/// let amount = wei(1.9); // returns 1
/// ```
fn wei(value: Dynamic) -> Result<BigInt, Box<rhai::EvalAltResult>> {
    scale_by_decimals(value, 0)
}

/// Converts a value to its atomic unit, assuming it is denominated in USDC (6
/// decimals).
///
/// # Arguments
///
/// * `value` - The value in USDC to convert.
///
/// # Example
///
/// ```rhai
/// let one_usdc = usdc(1); // returns 1000000
/// ```
fn usdc(value: Dynamic) -> Result<BigInt, Box<rhai::EvalAltResult>> {
    scale_by_decimals(value, 6)
}

/// Converts a value to its atomic unit, assuming it is denominated in USDT (6
/// decimals).
///
/// # Arguments
///
/// * `value` - The value in USDT to convert.
///
/// # Example
///
/// ```rhai
/// let one_usdt = usdt(1); // returns 1000000
/// ```
fn usdt(value: Dynamic) -> Result<BigInt, Box<rhai::EvalAltResult>> {
    scale_by_decimals(value, 6)
}

/// Converts a value to its atomic unit, assuming it is denominated in WBTC (8
/// decimals).
///
/// # Arguments
///
/// * `value` - The value in WBTC to convert.
///
/// # Example
///
/// ```rhai
/// let one_wbtc = wbtc(1); // returns 100000000
/// ```
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
    // Value is Decimal
    if value.is::<Decimal>() {
        return Ok(value.clone_cast::<Decimal>());
    }

    // To convert other types use their string representation:

    // Handle BigInt specifically
    if value.is::<BigInt>() {
        let bigint_value = value.clone_cast::<BigInt>();
        return Decimal::from_str(&bigint_value.to_string()).map_err(|_| {
            format!("Failed to convert BigInt value '{}' to Decimal", bigint_value).into()
        });
    }

    // Rest numeric type (i32, i64, f64) or string
    let string_repr = value.to_string();
    Decimal::from_str(&string_repr).map_err(|_| {
        format!(
            "Cannot convert value of type {:?} ('{}') to Decimal",
            value.type_name(),
            string_repr
        )
        .into()
    })
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
        let decimal = Decimal::new(123456789, 6);
        let dynamic_decimal = Dynamic::from(decimal);
        let dynamic_int = Dynamic::from(42);
        let dynamic_float = Dynamic::from(3.14);
        let large_bigint_str = "1234567890123456789012345"; // large, but can fit in a Decimal
        let dynamic_bigint = Dynamic::from(BigInt::from_str(large_bigint_str).unwrap());
        let dynamic_wrong_type = Dynamic::from("not a number");

        let decimal_decimal = dynamic_to_decimal(dynamic_decimal).unwrap();
        let decimal_int = dynamic_to_decimal(dynamic_int).unwrap();
        let decimal_float = dynamic_to_decimal(dynamic_float).unwrap();
        let decimal_bigint = dynamic_to_decimal(dynamic_bigint).unwrap();
        let decimal_wrong_type = dynamic_to_decimal(dynamic_wrong_type);

        assert_eq!(decimal_decimal, decimal);
        assert_eq!(decimal_int, Decimal::from(42));
        assert_eq!(decimal_float, Decimal::from_f64(3.14).unwrap());
        assert_eq!(decimal_bigint, Decimal::from_str(&large_bigint_str).unwrap());
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

    #[test]
    fn test_scale_by_decimals_truncation() {
        let dynamic_value = Dynamic::from(3.141592653589793);
        let scaled_value = scale_by_decimals(dynamic_value, 2).unwrap();
        assert_eq!(scaled_value, BigInt::from(314)); // Should truncate to 3.14
    }

    #[test]
    fn test_decimals_constructor_decimals_should_be_positive() {
        let dynamic_value = Dynamic::from(42);
        let neg_decimals = -6; // should be positive
        let result = decimals(dynamic_value, neg_decimals);
        assert!(result.is_err());
    }

    #[test]
    fn test_wei_constructor() {
        let dynamic_value = Dynamic::from(1.2);
        let wei_value = wei(dynamic_value).unwrap();
        assert_eq!(wei_value, BigInt::from(1)); // 1.2 wei should scale to 1 wei
    }

    #[test]
    fn test_gwei_constructor() {
        let dynamic_float = Dynamic::from(1.5);
        let dynamic_integer = Dynamic::from(2);
        let dynamic_string = Dynamic::from("3.0");
        let gwei_float = gwei(dynamic_float).unwrap();
        let gwei_integer = gwei(dynamic_integer).unwrap();
        let gwei_string = gwei(dynamic_string).unwrap();
        assert_eq!(gwei_float, BigInt::from(1500000000u64)); // 1.5 gwei should scale to 1.5 * 10^9 wei
        assert_eq!(gwei_integer, BigInt::from(2000000000u64)); // 2 gwei should scale to 2 * 10^9 wei
        assert_eq!(gwei_string, BigInt::from(3000000000u64)); // 3.0 gwei should scale to 3.0 * 10^9 wei
    }

    #[test]
    fn test_ether_constructor() {
        let dynamic_float = Dynamic::from(1.5);
        let dynamic_integer = Dynamic::from(2);
        let dynamic_string = Dynamic::from("3.0");
        let ether_float = ether(dynamic_float).unwrap();
        let ether_integer = ether(dynamic_integer).unwrap();
        let ether_string = ether(dynamic_string).unwrap();
        assert_eq!(ether_float, BigInt::from(1500000000000000000u64)); // 1.5 ether should scale to 1.5 * 10^18 wei
        assert_eq!(ether_integer, BigInt::from(2000000000000000000u64)); // 2 ether should scale to 2 * 10^18 wei
        assert_eq!(ether_string, BigInt::from(3000000000000000000u64)); // 3.0 ether should scale to 3.0 * 10^18 wei
    }

    #[test]
    fn test_usdc_constructor() {
        let dynamic_float = Dynamic::from(1.2345);
        let dynamic_integer = Dynamic::from(1000000000); // one billion
        let dynamic_string = Dynamic::from("1.2345");
        let usdc_float = usdc(dynamic_float).unwrap();
        let usdc_integer = usdc(dynamic_integer).unwrap();
        let usdc_string = usdc(dynamic_string).unwrap();
        assert_eq!(usdc_float, BigInt::from(1234500)); // 1.2345 USDC should scale to 1.2345 * 10^6
        assert_eq!(usdc_integer, BigInt::from(1000000000000000_i64)); // 1000000000 USDC should scale to 1000000000 * 10^6
        assert_eq!(usdc_string, BigInt::from(1234500)); // "1.2345" USDC should scale to 1.2345 * 10^6
    }

    #[test]
    fn test_usdt_constructor() {
        let dynamic_float = Dynamic::from(1.2345);
        let dynamic_integer = Dynamic::from(1000000000); // one billion
        let dynamic_string = Dynamic::from("1.2345");
        let usdt_float = usdt(dynamic_float).unwrap();
        let usdt_integer = usdt(dynamic_integer).unwrap();
        let usdt_string = usdt(dynamic_string).unwrap();
        assert_eq!(usdt_float, BigInt::from(1234500)); // 1.2345 USDT should scale to 1.2345 * 10^6
        assert_eq!(usdt_integer, BigInt::from(1000000000000000_i64)); // 1000000000 USDT should scale to 1000000000 * 10^6
        assert_eq!(usdt_string, BigInt::from(1234500)); // "1.2345" USDT should scale to 1.2345 * 10^6
    }

    #[test]
    fn test_wbtc_constructor() {
        let dynamic_float = Dynamic::from(0.12345678);
        let dynamic_integer = Dynamic::from(100000000); // one hundred million
        let dynamic_string = Dynamic::from("0.12345678");
        let wbtc_float = wbtc(dynamic_float).unwrap();
        let wbtc_integer = wbtc(dynamic_integer).unwrap();
        let wbtc_string = wbtc(dynamic_string).unwrap();
        assert_eq!(wbtc_float, BigInt::from(12345678)); // 0.12345678 WBTC should scale to 0.12345678 * 10^8
        assert_eq!(wbtc_integer, BigInt::from(10000000000000000_i64)); // 100000000 WBTC should scale to 100000000 * 10^8
        assert_eq!(wbtc_string, BigInt::from(12345678)); // "0.12345678" WBTC should scale to 0.12345678 * 10^8
    }
}
