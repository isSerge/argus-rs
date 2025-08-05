//! Universal operators for Rhai integration with UniversalNumber
//! 
//! This module provides operator functions that can be registered with the Rhai engine
//! to enable transparent arithmetic and comparison operations with any combination of
//! number types (i64, U256, I256, strings representing numbers).

use super::universal_number::UniversalNumber;
use rhai::{Dynamic, Engine, EvalAltResult};

/// Register all universal operators with the Rhai engine
/// 
/// This function registers arithmetic and comparison operators that work transparently
/// with any combination of number types, enabling seamless big number operations in scripts.
/// 
/// **Important**: This disables Fast Operators Mode to allow operator overriding.
pub fn register_universal_operators(engine: &mut Engine) {
    // Disable Fast Operators Mode to allow operator overriding
    engine.set_fast_operators(false);
    
    // Arithmetic operators
    register_arithmetic_operators(engine);
    
    // Comparison operators  
    register_comparison_operators(engine);
    
    // Utility functions
    register_utility_functions(engine);
}

/// Register arithmetic operators (+, -, *, /)
fn register_arithmetic_operators(engine: &mut Engine) {
    // Helper macro to reduce repetition
    macro_rules! register_mixed_arithmetic {
        ($op:literal, $func:ident) => {
            // INT op String
            engine.register_fn($op, |left: rhai::INT, right: String| -> Result<Dynamic, Box<EvalAltResult>> {
                $func(Dynamic::from(left), Dynamic::from(right))
            });
            
            // String op INT
            engine.register_fn($op, |left: String, right: rhai::INT| -> Result<Dynamic, Box<EvalAltResult>> {
                $func(Dynamic::from(left), Dynamic::from(right))
            });
            
            // String op String (for large number arithmetic)
            engine.register_fn($op, |left: String, right: String| -> Result<Dynamic, Box<EvalAltResult>> {
                $func(Dynamic::from(left), Dynamic::from(right))
            });
        };
    }
    
    // Register all arithmetic operators for mixed types
    register_mixed_arithmetic!("+", universal_add);
    register_mixed_arithmetic!("-", universal_subtract);
    register_mixed_arithmetic!("*", universal_multiply);
    register_mixed_arithmetic!("/", universal_divide);
}

/// Register comparison operators (==, !=, <, <=, >, >=)
fn register_comparison_operators(engine: &mut Engine) {
    // Helper macro to reduce repetition
    macro_rules! register_mixed_comparison {
        ($op:literal, $func:ident) => {
            // INT op String
            engine.register_fn($op, |left: rhai::INT, right: String| -> Result<bool, Box<EvalAltResult>> {
                $func(Dynamic::from(left), Dynamic::from(right))
            });
            
            // String op INT
            engine.register_fn($op, |left: String, right: rhai::INT| -> Result<bool, Box<EvalAltResult>> {
                $func(Dynamic::from(left), Dynamic::from(right))
            });
            
            // String op String (for numeric string comparison)
            engine.register_fn($op, |left: String, right: String| -> Result<bool, Box<EvalAltResult>> {
                $func(Dynamic::from(left), Dynamic::from(right))
            });
        };
    }
    
    // Register all comparison operators for mixed types
    register_mixed_comparison!("==", universal_equals);
    register_mixed_comparison!("!=", universal_not_equals);
    register_mixed_comparison!("<", universal_less_than);
    register_mixed_comparison!("<=", universal_less_than_or_equal);
    register_mixed_comparison!(">", universal_greater_than);
    register_mixed_comparison!(">=", universal_greater_than_or_equal);
}

/// Register utility functions for number handling
fn register_utility_functions(engine: &mut Engine) {
    // Helper macro to reduce repetition for utility functions
    macro_rules! register_utility {
        ($name:literal, $method:ident) => {
            engine.register_fn($name, |value: Dynamic| -> Result<bool, Box<EvalAltResult>> {
                let num = dynamic_to_universal_number(value)?;
                Ok(num.$method())
            });
        };
    }
    
    // Register utility functions
    register_utility!("is_zero", is_zero);
    register_utility!("is_positive", is_positive);
    register_utility!("is_negative", is_negative);
}

// =============================================================================
// Arithmetic Operation Functions
// =============================================================================

/// Universal addition: left + right
fn universal_add(left: Dynamic, right: Dynamic) -> Result<Dynamic, Box<EvalAltResult>> {
    let left_num = dynamic_to_universal_number(left)?;
    let right_num = dynamic_to_universal_number(right)?;
    
    let result = left_num.add(&right_num)
        .map_err(|e| format!("Addition failed: {}", e))?;
    
    Ok(result.to_dynamic())
}

/// Universal subtraction: left - right
fn universal_subtract(left: Dynamic, right: Dynamic) -> Result<Dynamic, Box<EvalAltResult>> {
    let left_num = dynamic_to_universal_number(left)?;
    let right_num = dynamic_to_universal_number(right)?;
    
    let result = left_num.sub(&right_num)
        .map_err(|e| format!("Subtraction failed: {}", e))?;
    
    Ok(result.to_dynamic())
}

/// Universal multiplication: left * right
fn universal_multiply(left: Dynamic, right: Dynamic) -> Result<Dynamic, Box<EvalAltResult>> {
    let left_num = dynamic_to_universal_number(left)?;
    let right_num = dynamic_to_universal_number(right)?;
    
    let result = left_num.mul(&right_num)
        .map_err(|e| format!("Multiplication failed: {}", e))?;
    
    Ok(result.to_dynamic())
}

/// Universal division: left / right
fn universal_divide(left: Dynamic, right: Dynamic) -> Result<Dynamic, Box<EvalAltResult>> {
    let left_num = dynamic_to_universal_number(left)?;
    let right_num = dynamic_to_universal_number(right)?;
    
    let result = left_num.div(&right_num)
        .map_err(|e| format!("Division failed: {}", e))?;
    
    Ok(result.to_dynamic())
}

// =============================================================================
// Comparison Operation Functions
// =============================================================================

/// Universal equality: left == right
fn universal_equals(left: Dynamic, right: Dynamic) -> Result<bool, Box<EvalAltResult>> {
    let left_num = dynamic_to_universal_number(left)?;
    let right_num = dynamic_to_universal_number(right)?;
    
    Ok(left_num == right_num)
}

/// Universal inequality: left != right
fn universal_not_equals(left: Dynamic, right: Dynamic) -> Result<bool, Box<EvalAltResult>> {
    let left_num = dynamic_to_universal_number(left)?;
    let right_num = dynamic_to_universal_number(right)?;
    
    Ok(left_num != right_num)
}

/// Universal less than: left < right
fn universal_less_than(left: Dynamic, right: Dynamic) -> Result<bool, Box<EvalAltResult>> {
    let left_num = dynamic_to_universal_number(left)?;
    let right_num = dynamic_to_universal_number(right)?;
    
    Ok(left_num < right_num)
}

/// Universal less than or equal: left <= right
fn universal_less_than_or_equal(left: Dynamic, right: Dynamic) -> Result<bool, Box<EvalAltResult>> {
    let left_num = dynamic_to_universal_number(left)?;
    let right_num = dynamic_to_universal_number(right)?;
    
    Ok(left_num <= right_num)
}

/// Universal greater than: left > right
fn universal_greater_than(left: Dynamic, right: Dynamic) -> Result<bool, Box<EvalAltResult>> {
    let left_num = dynamic_to_universal_number(left)?;
    let right_num = dynamic_to_universal_number(right)?;
    
    Ok(left_num > right_num)
}

/// Universal greater than or equal: left >= right
fn universal_greater_than_or_equal(left: Dynamic, right: Dynamic) -> Result<bool, Box<EvalAltResult>> {
    let left_num = dynamic_to_universal_number(left)?;
    let right_num = dynamic_to_universal_number(right)?;
    
    Ok(left_num >= right_num)
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Convert a Rhai Dynamic value to UniversalNumber
/// 
/// This function handles the conversion from various Rhai types (INT, String, etc.)
/// to UniversalNumber, providing transparent number handling in scripts.
fn dynamic_to_universal_number(value: Dynamic) -> Result<UniversalNumber, Box<EvalAltResult>> {
    UniversalNumber::from_dynamic(&value)
        .map_err(|e| format!("Failed to convert value to number: {}", e).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhai::{Dynamic, Engine};

    #[test]
    fn test_register_universal_operators() {
        let mut engine = Engine::new();
        register_universal_operators(&mut engine);
        
        // Test that operators are registered by compiling simple expressions
        assert!(engine.compile("1 + 2").is_ok());
        assert!(engine.compile("1 > 2").is_ok());
        assert!(engine.compile("1 == 2").is_ok());
    }

    #[test]
    fn test_universal_add_small_numbers() {
        let left = Dynamic::from(42i64);
        let right = Dynamic::from(58i64);
        
        let result = universal_add(left, right).unwrap();
        assert_eq!(result.as_int().unwrap(), 100);
    }

    #[test]
    fn test_universal_add_mixed_types() {
        let left = Dynamic::from(42i64);
        let right = Dynamic::from("58".to_string());
        
        let result = universal_add(left, right).unwrap();
        assert_eq!(result.as_int().unwrap(), 100);
    }

    #[test]
    fn test_universal_add_large_numbers() {
        let left = Dynamic::from("123456789012345678901234567890".to_string());
        let right = Dynamic::from("987654321098765432109876543210".to_string());
        
        let result = universal_add(left, right).unwrap();
        let result_str = result.into_string().unwrap();
        assert_eq!(result_str, "1111111110111111111011111111100");
    }

    #[test]
    fn test_universal_subtract() {
        let left = Dynamic::from(100i64);
        let right = Dynamic::from(42i64);
        
        let result = universal_subtract(left, right).unwrap();
        assert_eq!(result.as_int().unwrap(), 58);
    }

    #[test]
    fn test_universal_multiply() {
        let left = Dynamic::from(6i64);
        let right = Dynamic::from(7i64);
        
        let result = universal_multiply(left, right).unwrap();
        assert_eq!(result.as_int().unwrap(), 42);
    }

    #[test]
    fn test_universal_divide() {
        let left = Dynamic::from(42i64);
        let right = Dynamic::from(2i64);
        
        let result = universal_divide(left, right).unwrap();
        assert_eq!(result.as_int().unwrap(), 21);
    }

    #[test]
    fn test_universal_divide_by_zero() {
        let left = Dynamic::from(42i64);
        let right = Dynamic::from(0i64);
        
        let result = universal_divide(left, right);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Division failed"));
    }

    #[test]
    fn test_universal_equals() {
        let left = Dynamic::from(42i64);
        let right = Dynamic::from("42".to_string());
        
        let result = universal_equals(left, right).unwrap();
        assert!(result);
    }

    #[test]
    fn test_universal_not_equals() {
        let left = Dynamic::from(42i64);
        let right = Dynamic::from(43i64);
        
        let result = universal_not_equals(left, right).unwrap();
        assert!(result);
    }

    #[test]
    fn test_universal_less_than() {
        let left = Dynamic::from(42i64);
        let right = Dynamic::from("100".to_string());
        
        let result = universal_less_than(left, right).unwrap();
        assert!(result);
    }

    #[test]
    fn test_universal_greater_than() {
        let left = Dynamic::from("123456789012345678901234567890".to_string());
        let right = Dynamic::from(42i64);
        
        let result = universal_greater_than(left, right).unwrap();
        assert!(result);
    }

    #[test]
    fn test_universal_less_than_or_equal() {
        let left = Dynamic::from(42i64);
        let right = Dynamic::from(42i64);
        
        let result = universal_less_than_or_equal(left, right.clone()).unwrap();
        assert!(result);
        
        let left = Dynamic::from(41i64);
        let result = universal_less_than_or_equal(left, right).unwrap();
        assert!(result);
    }

    #[test]
    fn test_universal_greater_than_or_equal() {
        let left = Dynamic::from(42i64);
        let right = Dynamic::from(42i64);
        
        let result = universal_greater_than_or_equal(left, right.clone()).unwrap();
        assert!(result);
        
        let left = Dynamic::from(43i64);
        let result = universal_greater_than_or_equal(left, right).unwrap();
        assert!(result);
    }

    #[test]
    fn test_dynamic_to_universal_number_int() {
        let value = Dynamic::from(42i64);
        let num = dynamic_to_universal_number(value).unwrap();
        assert_eq!(num, UniversalNumber::Small(42));
    }

    #[test]
    fn test_dynamic_to_universal_number_string() {
        let value = Dynamic::from("123456789012345678901234567890".to_string());
        let num = dynamic_to_universal_number(value).unwrap();
        match num {
            UniversalNumber::BigUint(_) => {}, // Expected
            _ => panic!("Expected BigUint for large positive string"),
        }
    }

    #[test]
    fn test_dynamic_to_universal_number_invalid() {
        let value = Dynamic::from("not_a_number".to_string());
        let result = dynamic_to_universal_number(value);
        assert!(result.is_err());
    }

    #[test]
    fn test_rhai_integration_simple() {
        let mut engine = Engine::new();
        register_universal_operators(&mut engine);
        
        // Test simple arithmetic
        let result: i64 = engine.eval("42 + 58").unwrap();
        assert_eq!(result, 100);
        
        // Test comparison
        let result: bool = engine.eval("42 > 30").unwrap();
        assert!(result);
    }

    #[test]
    fn test_rhai_operator_registration_debug() {
        let mut engine = Engine::new();
        register_universal_operators(&mut engine);
        
        // Test direct function call first
        let script = r#"
            let val1 = 42;
            let val2 = "100";
            // For now, just test if we can call our function directly
            val1 == val1
        "#;
        
        let result: bool = engine.eval(script).unwrap();
        assert!(result);
    }

    #[test]
    fn test_mixed_type_operators_work() {
        let mut engine = Engine::new();
        register_universal_operators(&mut engine);
        
        // Test INT + String arithmetic
        let result: i64 = engine.eval("42 + \"100\"").unwrap();
        assert_eq!(result, 142);
        
        // Test String > INT comparison  
        let result: bool = engine.eval("\"1000\" > 42").unwrap();
        assert!(result);
        
        // Test String + INT
        let result: i64 = engine.eval("\"1000\" + 42").unwrap();
        assert_eq!(result, 1042);
        
        // Test large number arithmetic
        let result: String = engine.eval("\"123456789012345678901234567890\" + \"10\"").unwrap();
        assert_eq!(result, "123456789012345678901234567900");
    }
}
