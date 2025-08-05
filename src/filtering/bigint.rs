//! BigInt wrapper type for Rhai scripting with transparent big number handling
//! 
//! This module provides a BigInt wrapper that allows users to work with arbitrarily large
//! numbers in Rhai scripts while maintaining performance for standard integer operations.
//! Users wrap large numbers with `bigint()` and can then use standard operators seamlessly.

use alloy::primitives::U256;
use rhai::EvalAltResult;
use std::fmt;
use std::cmp::Ordering;
use thiserror::Error;

/// Errors that can occur during BigInt operations
#[derive(Debug, Error, PartialEq)]
pub enum BigIntError {
    /// Error parsing a string into a BigInt
    #[error("Failed to parse '{input}' as BigInt: {message}")]
    ParseError {
        /// The input string that failed to parse
        input: String,
        /// The underlying parsing error message
        message: String,
    },
    
    /// Arithmetic overflow during operation
    #[error("BigInt {operation} overflow")]
    ArithmeticOverflow {
        /// The operation that caused the overflow
        operation: String,
    },
    
    /// Division by zero
    #[error("Division by zero")]
    DivisionByZero,
}

/// BigInt wrapper for handling arbitrarily large numbers in Rhai scripts
/// 
/// This type can handle both signed and unsigned large integers, using U256 for storage
/// and a separate sign flag for negative values. This ensures we can represent the full
/// range of blockchain values (which are mostly unsigned) while still supporting arithmetic
/// operations that might result in negative values.
#[derive(Debug, Clone)]
pub struct BigInt {
    magnitude: U256,
    is_negative: bool,
}

impl BigInt {
    /// Create a new BigInt from a U256 magnitude and sign
    pub fn new(magnitude: U256, is_negative: bool) -> Self {
        Self { magnitude, is_negative }
    }
    
    /// Create a BigInt from an i64 value
    pub fn from_int(value: i64) -> Self {
        if value < 0 {
            Self::new(U256::from((-value) as u64), true)
        } else {
            Self::new(U256::from(value as u64), false)
        }
    }
    
    /// Create a BigInt from a string representation
    pub fn from_string(value: &str) -> Result<Self, BigIntError> {
        let trimmed = value.trim();
        
        if trimmed.starts_with('-') {
            // Negative number - parse without the minus sign
            let positive_str = &trimmed[1..];
            positive_str.parse::<U256>()
                .map(|magnitude| Self::new(magnitude, true))
                .map_err(|e| BigIntError::ParseError {
                    input: value.to_string(),
                    message: e.to_string(),
                })
        } else {
            // Positive number
            trimmed.parse::<U256>()
                .map(|magnitude| Self::new(magnitude, false))
                .map_err(|e| BigIntError::ParseError {
                    input: value.to_string(),
                    message: e.to_string(),
                })
        }
    }
    
    /// Check if the value is negative
    pub fn is_negative(&self) -> bool {
        self.is_negative && !self.magnitude.is_zero()
    }
    
    /// Check if the value is zero
    pub fn is_zero(&self) -> bool {
        self.magnitude.is_zero()
    }
}

// Arithmetic operations
impl BigInt {
    /// Add two BigInt values
    pub fn add(&self, other: &BigInt) -> Result<BigInt, BigIntError> {
        match (self.is_negative(), other.is_negative()) {
            (false, false) => {
                // Both positive: simple addition
                self.magnitude.checked_add(other.magnitude)
                    .map(|result| BigInt::new(result, false))
                    .ok_or(BigIntError::ArithmeticOverflow {
                        operation: "addition".to_string(),
                    })
            }
            (true, true) => {
                // Both negative: add magnitudes, result is negative
                self.magnitude.checked_add(other.magnitude)
                    .map(|result| BigInt::new(result, true))
                    .ok_or(BigIntError::ArithmeticOverflow {
                        operation: "addition".to_string(),
                    })
            }
            (false, true) => {
                // self positive, other negative: self - other.magnitude
                if self.magnitude >= other.magnitude {
                    Ok(BigInt::new(self.magnitude - other.magnitude, false))
                } else {
                    Ok(BigInt::new(other.magnitude - self.magnitude, true))
                }
            }
            (true, false) => {
                // self negative, other positive: other - self.magnitude
                if other.magnitude >= self.magnitude {
                    Ok(BigInt::new(other.magnitude - self.magnitude, false))
                } else {
                    Ok(BigInt::new(self.magnitude - other.magnitude, true))
                }
            }
        }
    }
    
    /// Subtract two BigInt values (self - other)
    pub fn sub(&self, other: &BigInt) -> Result<BigInt, BigIntError> {
        match (self.is_negative(), other.is_negative()) {
            (false, true) => { // self is positive, other is negative: self - (-other) = self + other
                self.magnitude.checked_add(other.magnitude)
                    .map(|result| BigInt::new(result, false))
                    .ok_or(BigIntError::ArithmeticOverflow {
                        operation: "subtraction".to_string(),
                    })
            }
            (true, false) => { // self is negative, other is positive: -self - other = -(self + other)
                self.magnitude.checked_add(other.magnitude)
                    .map(|result| BigInt::new(result, true))
                    .ok_or(BigIntError::ArithmeticOverflow {
                        operation: "subtraction".to_string(),
                    })
            }
            (false, false) => { // both positive: self - other
                if self.magnitude >= other.magnitude {
                    Ok(BigInt::new(self.magnitude - other.magnitude, false))
                } else {
                    Ok(BigInt::new(other.magnitude - self.magnitude, true))
                }
            }
            (true, true) => { // both negative: -self - (-other) = other - self
                if other.magnitude >= self.magnitude {
                    Ok(BigInt::new(other.magnitude - self.magnitude, false))
                } else {
                    Ok(BigInt::new(self.magnitude - other.magnitude, true))
                }
            }
        }
    }
    
    /// Multiply two BigInt values
    pub fn mul(&self, other: &BigInt) -> Result<BigInt, BigIntError> {
        if self.is_zero() || other.is_zero() {
            return Ok(BigInt::new(U256::ZERO, false));
        }
        
        self.magnitude.checked_mul(other.magnitude)
            .map(|result| {
                // Result is negative if exactly one operand is negative
                let result_negative = self.is_negative != other.is_negative;
                BigInt::new(result, result_negative)
            })
            .ok_or(BigIntError::ArithmeticOverflow {
                operation: "multiplication".to_string(),
            })
    }
    
    /// Divide two BigInt values
    pub fn div(&self, other: &BigInt) -> Result<BigInt, BigIntError> {
        if other.is_zero() {
            return Err(BigIntError::DivisionByZero);
        }
        
        if self.is_zero() {
            return Ok(BigInt::new(U256::ZERO, false));
        }
        
        self.magnitude.checked_div(other.magnitude)
            .map(|result| {
                // Result is negative if exactly one operand is negative
                let result_negative = self.is_negative != other.is_negative;
                BigInt::new(result, result_negative)
            })
            .ok_or(BigIntError::ArithmeticOverflow {
                operation: "division".to_string(),
            })
    }
}

// Implement comparison traits
impl PartialEq for BigInt {
    fn eq(&self, other: &Self) -> bool {
        if self.is_zero() && other.is_zero() {
            true // Both zero, regardless of sign flag
        } else {
            self.magnitude == other.magnitude && self.is_negative == other.is_negative
        }
    }
}

impl Eq for BigInt {}

impl PartialOrd for BigInt {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BigInt {
    fn cmp(&self, other: &Self) -> Ordering {
        // Handle zero cases first
        if self.is_zero() && other.is_zero() {
            return Ordering::Equal;
        }
        if self.is_zero() {
            return if other.is_negative { Ordering::Greater } else { Ordering::Less };
        }
        if other.is_zero() {
            return if self.is_negative { Ordering::Less } else { Ordering::Greater };
        }
        
        // Compare by sign first
        match (self.is_negative, other.is_negative) {
            (true, false) => Ordering::Less,    // negative < positive
            (false, true) => Ordering::Greater, // positive > negative
            (false, false) => self.magnitude.cmp(&other.magnitude), // both positive
            (true, true) => other.magnitude.cmp(&self.magnitude),   // both negative (reverse order)
        }
    }
}

impl fmt::Display for BigInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_negative && !self.is_zero() {
            write!(f, "-{}", self.magnitude)
        } else {
            write!(f, "{}", self.magnitude)
        }
    }
}

/// Constructor function for creating BigInt from various inputs
/// This will be registered with Rhai as the `bigint()` function
pub fn bigint_constructor_int(value: i64) -> BigInt {
    BigInt::from_int(value)
}

/// Constructor function for creating BigInt from string
pub fn bigint_constructor_string(value: String) -> Result<BigInt, Box<EvalAltResult>> {
    BigInt::from_string(&value)
        .map_err(|e| format!("Failed to create BigInt from string: {}", e).into())
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
        left.add(&right).map_err(|e| format!("BigInt addition failed: {}", e).into())
    });
    
    engine.register_fn("-", |left: BigInt, right: BigInt| -> Result<BigInt, Box<EvalAltResult>> {
        left.sub(&right).map_err(|e| format!("BigInt subtraction failed: {}", e).into())
    });
    
    engine.register_fn("*", |left: BigInt, right: BigInt| -> Result<BigInt, Box<EvalAltResult>> {
        left.mul(&right).map_err(|e| format!("BigInt multiplication failed: {}", e).into())
    });
    
    engine.register_fn("/", |left: BigInt, right: BigInt| -> Result<BigInt, Box<EvalAltResult>> {
        left.div(&right).map_err(|e| format!("BigInt division failed: {}", e).into())
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
    use super::*;
    use rhai::Engine;

    #[test]
    fn test_bigint_creation() {
        let big1 = BigInt::from_int(42);
        assert!(!big1.is_zero());
        assert!(!big1.is_negative());
        
        let big2 = BigInt::from_string("123456789012345678901234567890").unwrap();
        assert!(!big2.is_negative());
        
        let big3 = BigInt::from_string("-999999999999999999999999999999").unwrap();
        assert!(big3.is_negative());
    }
    
    #[test]
    fn test_bigint_arithmetic() {
        let big1 = BigInt::from_int(42);
        let big2 = BigInt::from_int(58);
        
        let result = big1.add(&big2).unwrap();
        assert_eq!(result.to_string(), "100");
        
        let result = big1.sub(&big2).unwrap();
        assert_eq!(result.to_string(), "-16");
        
        let result = big2.sub(&big1).unwrap();
        assert_eq!(result.to_string(), "16");

        let result = big1.mul(&big2).unwrap();
        assert_eq!(result.to_string(), "2436");
        
        let result = big2.div(&big1).unwrap();
        assert_eq!(result.to_string(), "1");
    }
    
    #[test]
    fn test_bigint_large_arithmetic() {
        let big1 = BigInt::from_string("123456789012345678901234567890").unwrap();
        let big2 = BigInt::from_string("987654321098765432109876543210").unwrap();
        
        let result = big1.add(&big2).unwrap();
        assert_eq!(result.to_string(), "1111111110111111111011111111100");
    }
    
    #[test]
    fn test_bigint_comparison() {
        let big1 = BigInt::from_int(42);
        let big2 = BigInt::from_int(100);
        let big3 = BigInt::from_int(42);
        
        assert!(big1 < big2);
        assert!(big2 > big1);
        assert!(big1 == big3);
        assert!(big1 <= big3);
        assert!(big1 >= big3);
        assert!(big1 != big2);
    }
    
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
        let result: BigInt = engine.eval("bigint(1000000000000000000) + bigint(2000000000000000000)").unwrap();
        assert_eq!(result.to_string(), "3000000000000000000");
        
        // Test subtraction
        let result: BigInt = engine.eval("bigint(5000000000000000000) - bigint(1000000000000000000)").unwrap();
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

    #[test]
    fn test_subtraction_scenarios() {
        // pos - neg = pos
        let a = BigInt::from_int(10);
        let b = BigInt::from_int(-5);
        let res = a.sub(&b).unwrap();
        assert_eq!(res.to_string(), "15");
        assert!(!res.is_negative());

        // neg - pos = neg
        let a = BigInt::from_int(-10);
        let b = BigInt::from_int(5);
        let res = a.sub(&b).unwrap();
        assert_eq!(res.to_string(), "-15");
        assert!(res.is_negative());

        // pos - pos (res pos)
        let a = BigInt::from_int(10);
        let b = BigInt::from_int(5);
        let res = a.sub(&b).unwrap();
        assert_eq!(res.to_string(), "5");
        assert!(!res.is_negative());

        // pos - pos (res neg)
        let a = BigInt::from_int(5);
        let b = BigInt::from_int(10);
        let res = a.sub(&b).unwrap();
        assert_eq!(res.to_string(), "-5");
        assert!(res.is_negative());

        // neg - neg (res pos)
        let a = BigInt::from_int(-5);
        let b = BigInt::from_int(-10);
        let res = a.sub(&b).unwrap();
        assert_eq!(res.to_string(), "5");
        assert!(!res.is_negative());

        // neg - neg (res neg)
        let a = BigInt::from_int(-10);
        let b = BigInt::from_int(-5);
        let res = a.sub(&b).unwrap();
        assert_eq!(res.to_string(), "-5");
        assert!(res.is_negative());
    }

    #[test]
    fn test_structured_errors() {
        // Test parsing error
        let parse_err = BigInt::from_string("not_a_number");
        assert!(parse_err.is_err());
        let err = parse_err.unwrap_err();
        match err {
            BigIntError::ParseError { input, message: _ } => {
                assert_eq!(input, "not_a_number");
            }
            _ => panic!("Expected ParseError"),
        }
        
        // Test division by zero error
        let zero = BigInt::from_int(0);
        let one = BigInt::from_int(1);
        let div_err = one.div(&zero);
        assert!(div_err.is_err());
        let err = div_err.unwrap_err();
        assert_eq!(err, BigIntError::DivisionByZero);
        
        // Test that we can format errors nicely
        assert_eq!(err.to_string(), "Division by zero");
        
        // Test error display for parse error
        let parse_err = BigIntError::ParseError {
            input: "abc".to_string(),
            message: "invalid digit found".to_string(),
        };
        assert_eq!(parse_err.to_string(), "Failed to parse 'abc' as BigInt: invalid digit found");
    }
}
