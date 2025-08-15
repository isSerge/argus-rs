//! BigInt wrapper type for Rhai scripting with transparent big number handling
//!
//! This module provides a BigInt wrapper that allows users to work with
//! arbitrarily large numbers in Rhai scripts while maintaining performance for
//! standard integer operations. Users wrap large numbers with `bigint()` and
//! can then use standard operators seamlessly.

use num_bigint::BigInt;
use num_traits::Zero;
use rhai::EvalAltResult;

/// Constructor function for creating BigInt from various inputs
/// This will be registered with Rhai as the `bigint()` function
pub fn bigint_constructor_int(value: i64) -> BigInt {
    value.into()
}

/// Constructor function for creating BigInt from string
pub fn bigint_constructor_string(value: String) -> Result<BigInt, Box<EvalAltResult>> {
    value.parse::<BigInt>().map_err(|e| format!("Failed to create BigInt from string: {e}").into())
}

/// Register BigInt type and operations with Rhai engine
pub fn register_bigint_with_rhai(engine: &mut rhai::Engine) {
    // Register the BigInt type
    engine.register_type_with_name::<BigInt>("BigInt");

    // Register constructors
    engine.register_fn("bigint", bigint_constructor_int);
    engine.register_fn("bigint", bigint_constructor_string);

    // Register arithmetic operators for BigInt
    engine.register_fn("+", |left: BigInt, right: BigInt| -> Result<BigInt, Box<EvalAltResult>> {
        Ok(left + right)
    });

    engine.register_fn("-", |left: BigInt, right: BigInt| -> Result<BigInt, Box<EvalAltResult>> {
        Ok(left - right)
    });

    engine.register_fn("*", |left: BigInt, right: BigInt| -> Result<BigInt, Box<EvalAltResult>> {
        Ok(left * right)
    });

    engine.register_fn("/", |left: BigInt, right: BigInt| -> Result<BigInt, Box<EvalAltResult>> {
        if right.is_zero() {
            return Err("Division by zero".into());
        }
        Ok(left / right)
    });

    // Register comparison operators for BigInt
    engine.register_fn("==", |left: BigInt, right: BigInt| left == right);
    engine.register_fn("!=", |left: BigInt, right: BigInt| left != right);
    engine.register_fn("<", |left: BigInt, right: BigInt| left < right);
    engine.register_fn("<=", |left: BigInt, right: BigInt| left <= right);
    engine.register_fn(">", |left: BigInt, right: BigInt| left > right);
    engine.register_fn(">=", |left: BigInt, right: BigInt| left >= right);
}

#[cfg(test)]
mod tests {
    use rhai::Engine;

    use super::*;

    #[test]
    fn test_rhai_integration() {
        let mut engine = Engine::new();
        register_bigint_with_rhai(&mut engine);

        // Test BigInt creation
        let result: BigInt = engine.eval("bigint(42)").unwrap();
        assert_eq!(result.to_string(), "42");

        let result: BigInt = engine.eval("bigint(\"123456789012345678901234567890\")").unwrap();
        assert_eq!(result.to_string(), "123456789012345678901234567890");

        // Test BigInt arithmetic (only BigInt + BigInt)
        let result: BigInt = engine.eval("bigint(42) + bigint(58)").unwrap();
        assert_eq!(result.to_string(), "100");

        // Test BigInt comparison (only BigInt with BigInt)
        let result: bool = engine.eval("bigint(50) > bigint(42)").unwrap();
        assert!(result);

        let result: bool = engine.eval("bigint(42) == bigint(42)").unwrap();
        assert!(result);

        let result: bool = engine.eval("bigint(42) != bigint(100)").unwrap();
        assert!(result);
    }

    #[test]
    fn test_core_functionality() {
        let mut engine = Engine::new();
        register_bigint_with_rhai(&mut engine);

        // Test basic arithmetic expressions
        let result: BigInt =
            engine.eval("bigint(1000000000000000000) + bigint(2000000000000000000)").unwrap();
        assert_eq!(result.to_string(), "3000000000000000000");

        // Test subtraction
        let result: BigInt =
            engine.eval("bigint(5000000000000000000) - bigint(1000000000000000000)").unwrap();
        assert_eq!(result.to_string(), "4000000000000000000");

        // Test multiplication
        let result: BigInt = engine.eval("bigint(1000000) * bigint(1000000)").unwrap();
        assert_eq!(result.to_string(), "1000000000000");

        // Test division
        let result: BigInt = engine.eval("bigint(1000000000000) / bigint(1000000)").unwrap();
        assert_eq!(result.to_string(), "1000000");
    }

    #[test]
    fn test_error_handling() {
        let mut engine = Engine::new();
        register_bigint_with_rhai(&mut engine);

        // Test invalid string
        let result = engine.eval::<BigInt>("bigint(\"not_a_number\")");
        assert!(result.is_err());

        // Test division by zero
        let result = engine.eval::<BigInt>("bigint(42) / bigint(0)");
        assert!(result.is_err());
    }
}
