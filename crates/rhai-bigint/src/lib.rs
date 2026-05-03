//! Arbitrary-precision BigInt support for Rhai scripts.
//!
//! Provides [`BigIntPackage`] (via `def_package!`) and the
//! `register_bigint_with_rhai` convenience function.

use num_bigint::BigInt;
use rhai::{def_package, packages::Package, plugin::*};

#[export_module]
mod bigint_functions {
    use num_bigint::BigInt;
    use num_traits::Zero;

    /// Creates a `BigInt` from an integer.
    pub fn bigint(value: i64) -> BigInt {
        value.into()
    }

    /// Creates a `BigInt` from a string.
    #[rhai_fn(name = "bigint", return_raw)]
    pub fn bigint_from_str(value: String) -> Result<BigInt, Box<rhai::EvalAltResult>> {
        value
            .parse::<BigInt>()
            .map_err(|e| format!("Failed to create BigInt from string: {e}").into())
    }

    #[rhai_fn(name = "+", pure)]
    pub fn add(l: &mut BigInt, r: BigInt) -> BigInt {
        l.clone() + r
    }

    #[rhai_fn(name = "-", pure)]
    pub fn sub(l: &mut BigInt, r: BigInt) -> BigInt {
        l.clone() - r
    }

    #[rhai_fn(name = "*", pure)]
    pub fn mul(l: &mut BigInt, r: BigInt) -> BigInt {
        l.clone() * r
    }

    #[rhai_fn(name = "/", pure, return_raw)]
    pub fn div(l: &mut BigInt, r: BigInt) -> Result<BigInt, Box<rhai::EvalAltResult>> {
        if r.is_zero() {
            return Err("Division by zero".into());
        }
        Ok(l.clone() / r)
    }

    #[rhai_fn(name = "==", pure)]
    pub fn eq(l: &mut BigInt, r: BigInt) -> bool {
        *l == r
    }

    #[rhai_fn(name = "!=", pure)]
    pub fn ne(l: &mut BigInt, r: BigInt) -> bool {
        *l != r
    }

    #[rhai_fn(name = "<", pure)]
    pub fn lt(l: &mut BigInt, r: BigInt) -> bool {
        *l < r
    }

    #[rhai_fn(name = "<=", pure)]
    pub fn le(l: &mut BigInt, r: BigInt) -> bool {
        *l <= r
    }

    #[rhai_fn(name = ">", pure)]
    pub fn gt(l: &mut BigInt, r: BigInt) -> bool {
        *l > r
    }

    #[rhai_fn(name = ">=", pure)]
    pub fn ge(l: &mut BigInt, r: BigInt) -> bool {
        *l >= r
    }
}

def_package! {
    /// Arbitrary-precision BigInt for Rhai: `bigint()` constructor plus
    /// arithmetic (`+`, `-`, `*`, `/`) and comparison operators.
    pub BigIntPackage(lib) {
        combine_with_exported_module!(lib, "bigint", bigint_functions);
    }
}

/// Register the `BigInt` custom type and [`BigIntPackage`] with a Rhai engine.
pub fn register_bigint_with_rhai(engine: &mut rhai::Engine) {
    engine.register_type_with_name::<BigInt>("BigInt");
    BigIntPackage::new().register_into_engine(engine);
}

#[cfg(test)]
mod tests {
    use rhai::Engine;

    use super::*;

    #[test]
    fn test_rhai_integration() {
        let mut engine = Engine::new();
        register_bigint_with_rhai(&mut engine);

        let result: BigInt = engine.eval("bigint(42)").unwrap();
        assert_eq!(result.to_string(), "42");

        let result: BigInt = engine.eval("bigint(\"123456789012345678901234567890\")").unwrap();
        assert_eq!(result.to_string(), "123456789012345678901234567890");

        let result: BigInt = engine.eval("bigint(42) + bigint(58)").unwrap();
        assert_eq!(result.to_string(), "100");

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

        let result: BigInt =
            engine.eval("bigint(1000000000000000000) + bigint(2000000000000000000)").unwrap();
        assert_eq!(result.to_string(), "3000000000000000000");

        let result: BigInt =
            engine.eval("bigint(5000000000000000000) - bigint(1000000000000000000)").unwrap();
        assert_eq!(result.to_string(), "4000000000000000000");

        let result: BigInt = engine.eval("bigint(1000000) * bigint(1000000)").unwrap();
        assert_eq!(result.to_string(), "1000000000000");

        let result: BigInt = engine.eval("bigint(1000000000000) / bigint(1000000)").unwrap();
        assert_eq!(result.to_string(), "1000000");
    }

    #[test]
    fn test_error_handling() {
        let mut engine = Engine::new();
        register_bigint_with_rhai(&mut engine);

        let result = engine.eval::<BigInt>("bigint(\"not_a_number\")");
        assert!(result.is_err());

        let result = engine.eval::<BigInt>("bigint(42) / bigint(0)");
        assert!(result.is_err());
    }
}
