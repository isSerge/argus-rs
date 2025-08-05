//! BigInt wrapper type for Rhai scripting with transparent big number handling
//! 
//! This module provides a BigInt wrapper that allows users to work with arbitrarily large
//! numbers in Rhai scripts while maintaining performance for standard integer operations.
//! Users wrap large numbers with `bigint()` and can then use standard operators seamlessly.

use alloy::primitives::{I256, U256};
use rhai::{Dynamic, EvalAltResult};
use std::fmt;
use std::cmp::Ordering;

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
    
    /// Create a BigInt from a U256 value (always positive)
    pub fn from_u256(value: U256) -> Self {
        Self::new(value, false)
    }
    
    /// Create a BigInt from an I256 value (preserving sign)
    pub fn from_i256(value: I256) -> Self {
        if value.is_negative() {
            // Convert negative I256 to positive U256 magnitude
            let magnitude = U256::try_from(-value).unwrap_or(U256::ZERO);
            Self::new(magnitude, true)
        } else {
            let magnitude = U256::try_from(value).unwrap_or(U256::ZERO);
            Self::new(magnitude, false)
        }
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
    pub fn from_string(value: &str) -> Result<Self, String> {
        let trimmed = value.trim();
        
        if trimmed.starts_with('-') {
            // Negative number - parse without the minus sign
            let positive_str = &trimmed[1..];
            positive_str.parse::<U256>()
                .map(|magnitude| Self::new(magnitude, true))
                .map_err(|e| format!("Invalid negative number '{}': {}", value, e))
        } else {
            // Positive number
            trimmed.parse::<U256>()
                .map(|magnitude| Self::new(magnitude, false))
                .map_err(|e| format!("Invalid positive number '{}': {}", value, e))
        }
    }
    
    /// Get the magnitude (absolute value) as U256
    pub fn magnitude(&self) -> U256 {
        self.magnitude
    }
    
    /// Check if the value is negative
    pub fn is_negative(&self) -> bool {
        self.is_negative && !self.magnitude.is_zero()
    }
    
    /// Check if the value is positive
    pub fn is_positive(&self) -> bool {
        !self.is_negative && !self.magnitude.is_zero()
    }
    
    /// Check if the value is zero
    pub fn is_zero(&self) -> bool {
        self.magnitude.is_zero()
    }
    
    /// Convert to Dynamic for returning to Rhai
    pub fn to_dynamic(&self) -> Dynamic {
        // Try to fit in i64 first
        if let Some(i64_val) = self.to_i64() {
            Dynamic::from(i64_val)
        } else {
            // Return as string for large values
            Dynamic::from(self.to_string())
        }
    }
    
    /// Try to convert to i64, returning None if out of range
    pub fn to_i64(&self) -> Option<i64> {
        if self.magnitude <= U256::from(i64::MAX as u64) {
            let magnitude_i64 = self.magnitude.to::<u64>() as i64;
            if self.is_negative {
                Some(-magnitude_i64)
            } else {
                Some(magnitude_i64)
            }
        } else {
            None
        }
    }
    
    /// Convert to I256 if possible
    pub fn to_i256(&self) -> Option<I256> {
        if self.is_negative {
            // For negative values, check if magnitude fits in I256 positive range
            if self.magnitude <= U256::try_from(I256::MAX).unwrap_or(U256::ZERO) {
                I256::try_from(self.magnitude).ok().map(|val| -val)
            } else {
                None // Too large negative value for I256
            }
        } else {
            // For positive values, convert directly
            I256::try_from(self.magnitude).ok()
        }
    }
    
    /// Convert to U256 if positive, None if negative
    pub fn to_u256(&self) -> Option<U256> {
        if self.is_negative {
            None
        } else {
            Some(self.magnitude)
        }
    }
    
    /// Convert to string representation
    pub fn to_string(&self) -> String {
        if self.is_negative && !self.is_zero() {
            format!("-{}", self.magnitude)
        } else {
            self.magnitude.to_string()
        }
    }
}

// Arithmetic operations
impl BigInt {
    /// Add two BigInt values
    pub fn add(&self, other: &BigInt) -> Result<BigInt, String> {
        match (self.is_negative, other.is_negative) {
            (false, false) => {
                // Both positive: simple addition
                self.magnitude.checked_add(other.magnitude)
                    .map(|result| BigInt::new(result, false))
                    .ok_or_else(|| "BigInt addition overflow".to_string())
            }
            (true, true) => {
                // Both negative: add magnitudes, result is negative
                self.magnitude.checked_add(other.magnitude)
                    .map(|result| BigInt::new(result, true))
                    .ok_or_else(|| "BigInt addition overflow".to_string())
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
    pub fn sub(&self, other: &BigInt) -> Result<BigInt, String> {
        // Subtraction is addition with negated second operand
        let negated_other = BigInt::new(other.magnitude, !other.is_negative);
        self.add(&negated_other)
    }
    
    /// Multiply two BigInt values
    pub fn mul(&self, other: &BigInt) -> Result<BigInt, String> {
        if self.is_zero() || other.is_zero() {
            return Ok(BigInt::new(U256::ZERO, false));
        }
        
        self.magnitude.checked_mul(other.magnitude)
            .map(|result| {
                // Result is negative if exactly one operand is negative
                let result_negative = self.is_negative != other.is_negative;
                BigInt::new(result, result_negative)
            })
            .ok_or_else(|| "BigInt multiplication overflow".to_string())
    }
    
    /// Divide two BigInt values
    pub fn div(&self, other: &BigInt) -> Result<BigInt, String> {
        if other.is_zero() {
            return Err("Division by zero".to_string());
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
            .ok_or_else(|| "BigInt division overflow".to_string())
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
        .map_err(|e| format!("Failed to create BigInt from string '{}': {}", value, e).into())
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
    use std::str::FromStr;

    #[test]
    fn test_bigint_creation() {
        let big1 = BigInt::from_int(42);
        assert!(!big1.is_zero());
        assert!(big1.is_positive());
        assert!(!big1.is_negative());
        
        let big2 = BigInt::from_string("123456789012345678901234567890").unwrap();
        assert!(big2.is_positive());
        
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
    fn test_large_u256_values() {
        // Test with U256::MAX (largest possible U256 value)
        let u256_max_str = U256::MAX.to_string();
        let big_u256_max = BigInt::from_string(&u256_max_str).unwrap();
        assert!(big_u256_max.is_positive());
        assert_eq!(big_u256_max.to_string(), u256_max_str);
        
        // Test with a value larger than I256::MAX but smaller than U256::MAX
        let large_u256 = U256::from_str("57896044618658097711785492504343953926634992332820282019728792003956564819968").unwrap(); // 2^255
        let big_large = BigInt::from_u256(large_u256);
        assert!(big_large.is_positive());
        assert_eq!(big_large.magnitude(), large_u256);
        
        // Test arithmetic with very large values
        let big1 = BigInt::from_string("340282366920938463463374607431768211455").unwrap(); // 2^128 - 1
        let big2 = BigInt::from_string("340282366920938463463374607431768211455").unwrap();
        let result = big1.add(&big2).unwrap();
        assert_eq!(result.to_string(), "680564733841876926926749214863536422910"); // 2^129 - 2
    }
}
