//! Universal number type that handles blockchain values of any size transparently

use alloy::primitives::{U256, I256};
use rhai::Dynamic;
use std::cmp::Ordering;
use std::convert::TryFrom;
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

/// Errors that can occur during big number operations
#[derive(Debug, Error, Clone, PartialEq)]
pub enum BigNumberError {
    /// Invalid number format in string input
    #[error("Invalid number format: '{input}'. Expected a valid integer or decimal number.")]
    InvalidFormat { input: String },
    
    /// Arithmetic operation caused overflow
    #[error("Arithmetic overflow in {operation}: {details}")]
    ArithmeticOverflow { operation: String, details: String },
    
    /// Division by zero attempted
    #[error("Division by zero")]
    DivisionByZero,
    
    /// Number too large for the requested operation
    #[error("Number too large for operation: {operation}")]
    NumberTooLarge { operation: String },
    
    /// Unsupported operation between incompatible types
    #[error("Unsupported operation: {operation} between {left_type} and {right_type}")]
    UnsupportedOperation {
        operation: String,
        left_type: String,
        right_type: String,
    },
    
    /// Conversion error between types
    #[error("Conversion error: {details}")]
    ConversionError { details: String },
    
    /// Parsing error for numeric strings
    #[error("Parse error: {details}")]
    ParseError { details: String },
}

/// Result type for big number operations
pub type BigNumberResult<T> = Result<T, BigNumberError>;

/// A universal number type that can represent values of any size
/// and supports transparent operations between different number types.
#[derive(Debug, Clone, PartialEq)]
pub enum UniversalNumber {
    /// Small integers that fit in i64 (most common case)
    Small(i64),
    /// Large unsigned integers (positive values > i64::MAX)
    BigUint(U256),
    /// Large signed integers (negative values < i64::MIN or positive > i64::MAX)
    BigInt(I256),
}

impl UniversalNumber {
    /// Create a UniversalNumber from an i64 value
    pub fn from_i64(value: i64) -> Self {
        UniversalNumber::Small(value)
    }

    /// Create a UniversalNumber from a u64 value
    pub fn from_u64(value: u64) -> Self {
        // If the u64 fits in i64, use Small variant for efficiency
        if value <= i64::MAX as u64 {
            UniversalNumber::Small(value as i64)
        } else {
            // Value is too large for i64, use BigUint
            UniversalNumber::BigUint(U256::from(value))
        }
    }

    /// Create a UniversalNumber from a U256 value
    pub fn from_u256(value: U256) -> Self {
        // Try to fit in Small variant if possible for efficiency
        if value <= U256::from(i64::MAX) {
            UniversalNumber::Small(value.to::<i64>())
        } else {
            UniversalNumber::BigUint(value)
        }
    }

    /// Create a UniversalNumber from an I256 value
    pub fn from_i256(value: I256) -> Self {
        // Try to fit in Small variant if possible for efficiency
        // Check if the I256 value fits within i64 bounds (both positive and negative)
        if let Ok(small_value) = i64::try_from(value) {
            UniversalNumber::Small(small_value)
        } else {
            UniversalNumber::BigInt(value)
        }
    }
    
    /// Create a UniversalNumber from a string representation
    /// Supports decimal integers (e.g., "123", "-456")
    pub fn from_string(s: &str) -> Result<Self, BigNumberError> {
        let s = s.trim();
        
        // Handle empty strings
        if s.is_empty() {
            return Err(BigNumberError::ParseError { details: "Empty string".to_string() });
        }
        
        // Handle decimal strings (both positive and negative)
        if s.starts_with('-') {
            // Negative number - try I256
            match s.parse::<I256>() {
                Ok(value) => Ok(Self::from_i256(value)),
                Err(_) => Err(BigNumberError::ParseError { details: format!("Invalid decimal string: {}", s) })
            }
        } else {
            // Positive number - try U256 first for maximum range
            match s.parse::<U256>() {
                Ok(value) => Ok(Self::from_u256(value)),
                Err(_) => Err(BigNumberError::ParseError { details: format!("Invalid decimal string: {}", s) })
            }
        }
    }
    
    /// Create a UniversalNumber from a Rhai Dynamic value
    /// Supports INT (i64), strings, and other numeric types
    pub fn from_dynamic(value: &rhai::Dynamic) -> Result<Self, BigNumberError> {
        // Handle different Dynamic value types
        if value.is::<rhai::INT>() {
            // Rhai INT is typically i64
            let int_val = value.as_int().map_err(|e| 
                BigNumberError::ConversionError { details: format!("Failed to extract INT: {}", e) })?;
            Ok(Self::from_i64(int_val))
        } else if value.is::<String>() {
            // Handle string representation
            let string_val = value.clone().into_string().map_err(|e|
                BigNumberError::ConversionError { details: format!("Failed to extract string: {}", e) })?;
            Self::from_string(&string_val)
        } else if value.is::<&str>() {
            // Handle string slice - convert to string first
            let string_repr = value.to_string();
            Self::from_string(&string_repr)
        } else {
            // Try to convert to string as a fallback
            let string_repr = value.to_string();
            Self::from_string(&string_repr)
        }
    }
    
    /// Convert a UniversalNumber to a Rhai Dynamic value
    /// Large numbers are converted to strings to preserve precision
    pub fn to_dynamic(&self) -> rhai::Dynamic {
        match self {
            UniversalNumber::Small(value) => (*value).into(),
            UniversalNumber::BigUint(value) => value.to_string().into(),
            UniversalNumber::BigInt(value) => value.to_string().into(),
        }
    }

    /// Check if the number is zero
    pub fn is_zero(&self) -> bool {
        match self {
            UniversalNumber::Small(value) => *value == 0,
            UniversalNumber::BigUint(value) => value.is_zero(),
            UniversalNumber::BigInt(value) => value.is_zero(),
        }
    }

    /// Check if the number is positive (greater than zero)
    pub fn is_positive(&self) -> bool {
        match self {
            UniversalNumber::Small(value) => *value > 0,
            UniversalNumber::BigUint(value) => !value.is_zero(), // U256 is always non-negative, so non-zero means positive
            UniversalNumber::BigInt(value) => value.is_positive(),
        }
    }

    /// Check if the number is negative (less than zero)
    pub fn is_negative(&self) -> bool {
        match self {
            UniversalNumber::Small(value) => *value < 0,
            UniversalNumber::BigUint(_) => false, // U256 is always non-negative
            UniversalNumber::BigInt(value) => value.is_negative(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::{U256, I256};

    #[test]
    fn test_from_i64() {
        // Test positive i64
        let num = UniversalNumber::from_i64(42);
        assert_eq!(num, UniversalNumber::Small(42));

        // Test negative i64
        let num = UniversalNumber::from_i64(-123);
        assert_eq!(num, UniversalNumber::Small(-123));

        // Test zero
        let num = UniversalNumber::from_i64(0);
        assert_eq!(num, UniversalNumber::Small(0));

        // Test max i64
        let num = UniversalNumber::from_i64(i64::MAX);
        assert_eq!(num, UniversalNumber::Small(i64::MAX));

        // Test min i64
        let num = UniversalNumber::from_i64(i64::MIN);
        assert_eq!(num, UniversalNumber::Small(i64::MIN));
    }

    #[test]
    fn test_from_u64() {
        // Test small u64 that fits in i64
        let num = UniversalNumber::from_u64(42);
        assert_eq!(num, UniversalNumber::Small(42));

        // Test u64 at i64::MAX boundary
        let num = UniversalNumber::from_u64(i64::MAX as u64);
        assert_eq!(num, UniversalNumber::Small(i64::MAX));

        // Test u64 just above i64::MAX
        let large_u64 = (i64::MAX as u64) + 1;
        let num = UniversalNumber::from_u64(large_u64);
        assert_eq!(num, UniversalNumber::BigUint(U256::from(large_u64)));

        // Test u64::MAX
        let num = UniversalNumber::from_u64(u64::MAX);
        assert_eq!(num, UniversalNumber::BigUint(U256::from(u64::MAX)));

        // Test zero
        let num = UniversalNumber::from_u64(0);
        assert_eq!(num, UniversalNumber::Small(0));
    }

    #[test]
    fn test_from_u256() {
        // Test small U256 that fits in i64
        let small_u256 = U256::from(42);
        let num = UniversalNumber::from_u256(small_u256);
        assert_eq!(num, UniversalNumber::Small(42));

        // Test U256 at i64::MAX boundary
        let max_i64_u256 = U256::from(i64::MAX);
        let num = UniversalNumber::from_u256(max_i64_u256);
        assert_eq!(num, UniversalNumber::Small(i64::MAX));

        // Test large U256 that doesn't fit in i64
        let large_u256 = U256::from(u64::MAX);
        let num = UniversalNumber::from_u256(large_u256);
        assert_eq!(num, UniversalNumber::BigUint(large_u256));

        // Test very large U256
        let very_large = U256::from_str("123456789012345678901234567890").unwrap();
        let num = UniversalNumber::from_u256(very_large);
        assert_eq!(num, UniversalNumber::BigUint(very_large));

        // Test zero
        let num = UniversalNumber::from_u256(U256::ZERO);
        assert_eq!(num, UniversalNumber::Small(0));
    }

    #[test]
    fn test_from_i256() {
        // Test small positive I256 that fits in i64
        let small_positive = I256::try_from(42).unwrap();
        let num = UniversalNumber::from_i256(small_positive);
        assert_eq!(num, UniversalNumber::Small(42));

        // Test small negative I256 that fits in i64
        let small_negative = I256::try_from(-42).unwrap();
        let num = UniversalNumber::from_i256(small_negative);
        assert_eq!(num, UniversalNumber::Small(-42));

        // Test I256 at i64::MAX boundary
        let max_i64_i256 = I256::try_from(i64::MAX).unwrap();
        let num = UniversalNumber::from_i256(max_i64_i256);
        assert_eq!(num, UniversalNumber::Small(i64::MAX));

        // Test I256 at i64::MIN boundary
        let min_i64_i256 = I256::try_from(i64::MIN).unwrap();
        let num = UniversalNumber::from_i256(min_i64_i256);
        assert_eq!(num, UniversalNumber::Small(i64::MIN));

        // Test large positive I256 that doesn't fit in i64
        let large_positive = I256::from_str("123456789012345678901234567890").unwrap();
        let num = UniversalNumber::from_i256(large_positive);
        assert_eq!(num, UniversalNumber::BigInt(large_positive));

        // Test large negative I256 that doesn't fit in i64
        let large_negative = I256::from_str("-123456789012345678901234567890").unwrap();
        let num = UniversalNumber::from_i256(large_negative);
        assert_eq!(num, UniversalNumber::BigInt(large_negative));

        // Test zero
        let num = UniversalNumber::from_i256(I256::ZERO);
        assert_eq!(num, UniversalNumber::Small(0));
    }

    #[test]
    fn test_is_zero() {
        // Test Small zero
        let num = UniversalNumber::Small(0);
        assert!(num.is_zero());

        // Test Small non-zero positive
        let num = UniversalNumber::Small(42);
        assert!(!num.is_zero());

        // Test Small non-zero negative
        let num = UniversalNumber::Small(-42);
        assert!(!num.is_zero());

        // Test BigUint zero
        let num = UniversalNumber::BigUint(U256::ZERO);
        assert!(num.is_zero());

        // Test BigUint non-zero
        let num = UniversalNumber::BigUint(U256::from(123));
        assert!(!num.is_zero());

        // Test BigInt zero
        let num = UniversalNumber::BigInt(I256::ZERO);
        assert!(num.is_zero());

        // Test BigInt non-zero positive
        let large_positive = I256::from_str("123456789012345678901234567890").unwrap();
        let num = UniversalNumber::BigInt(large_positive);
        assert!(!num.is_zero());

        // Test BigInt non-zero negative
        let large_negative = I256::from_str("-123456789012345678901234567890").unwrap();
        let num = UniversalNumber::BigInt(large_negative);
        assert!(!num.is_zero());
    }

    #[test]
    fn test_is_positive() {
        // Test Small positive
        let num = UniversalNumber::Small(42);
        assert!(num.is_positive());

        // Test Small zero (not positive)
        let num = UniversalNumber::Small(0);
        assert!(!num.is_positive());

        // Test Small negative
        let num = UniversalNumber::Small(-42);
        assert!(!num.is_positive());

        // Test BigUint positive
        let num = UniversalNumber::BigUint(U256::from(123));
        assert!(num.is_positive());

        // Test BigUint zero (not positive)
        let num = UniversalNumber::BigUint(U256::ZERO);
        assert!(!num.is_positive());

        // Test BigInt positive
        let large_positive = I256::from_str("123456789012345678901234567890").unwrap();
        let num = UniversalNumber::BigInt(large_positive);
        assert!(num.is_positive());

        // Test BigInt zero (not positive)
        let num = UniversalNumber::BigInt(I256::ZERO);
        assert!(!num.is_positive());

        // Test BigInt negative
        let large_negative = I256::from_str("-123456789012345678901234567890").unwrap();
        let num = UniversalNumber::BigInt(large_negative);
        assert!(!num.is_positive());
    }

    #[test]
    fn test_is_negative() {
        // Test Small positive
        let num = UniversalNumber::Small(42);
        assert!(!num.is_negative());

        // Test Small zero (not negative)
        let num = UniversalNumber::Small(0);
        assert!(!num.is_negative());

        // Test Small negative
        let num = UniversalNumber::Small(-42);
        assert!(num.is_negative());

        // Test BigUint (never negative)
        let num = UniversalNumber::BigUint(U256::from(123));
        assert!(!num.is_negative());

        // Test BigUint zero (not negative)
        let num = UniversalNumber::BigUint(U256::ZERO);
        assert!(!num.is_negative());

        // Test BigInt positive
        let large_positive = I256::from_str("123456789012345678901234567890").unwrap();
        let num = UniversalNumber::BigInt(large_positive);
        assert!(!num.is_negative());

        // Test BigInt zero (not negative)
        let num = UniversalNumber::BigInt(I256::ZERO);
        assert!(!num.is_negative());

        // Test BigInt negative
        let large_negative = I256::from_str("-123456789012345678901234567890").unwrap();
        let num = UniversalNumber::BigInt(large_negative);
        assert!(num.is_negative());
    }

    #[test]
    fn test_to_dynamic() {
        // Test Small positive
        let num = UniversalNumber::Small(42);
        let dynamic = num.to_dynamic();
        assert_eq!(dynamic.as_int().unwrap(), 42);

        // Test Small negative
        let num = UniversalNumber::Small(-123);
        let dynamic = num.to_dynamic();
        assert_eq!(dynamic.as_int().unwrap(), -123);

        // Test Small zero
        let num = UniversalNumber::Small(0);
        let dynamic = num.to_dynamic();
        assert_eq!(dynamic.as_int().unwrap(), 0);

        // Test BigUint (should become string)
        let large_u256 = U256::from_str("123456789012345678901234567890").unwrap();
        let num = UniversalNumber::BigUint(large_u256);
        let dynamic = num.to_dynamic();
        let string_val = dynamic.into_string().unwrap();
        assert_eq!(string_val, "123456789012345678901234567890");

        // Test BigInt positive (should become string)
        let large_positive = I256::from_str("123456789012345678901234567890").unwrap();
        let num = UniversalNumber::BigInt(large_positive);
        let dynamic = num.to_dynamic();
        let string_val = dynamic.into_string().unwrap();
        assert_eq!(string_val, "123456789012345678901234567890");

        // Test BigInt negative (should become string)
        let large_negative = I256::from_str("-123456789012345678901234567890").unwrap();
        let num = UniversalNumber::BigInt(large_negative);
        let dynamic = num.to_dynamic();
        let string_val = dynamic.into_string().unwrap();
        assert_eq!(string_val, "-123456789012345678901234567890");
    }

    #[test]
    fn test_from_string() {
        // Test positive decimal string that fits in i64
        let result = UniversalNumber::from_string("42");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), UniversalNumber::Small(42));

        // Test negative decimal string that fits in i64
        let result = UniversalNumber::from_string("-123");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), UniversalNumber::Small(-123));

        // Test zero
        let result = UniversalNumber::from_string("0");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), UniversalNumber::Small(0));

        // Test large positive decimal string
        let result = UniversalNumber::from_string("123456789012345678901234567890");
        assert!(result.is_ok());
        match result.unwrap() {
            UniversalNumber::BigUint(value) => {
                assert_eq!(value, U256::from_str("123456789012345678901234567890").unwrap());
            }
            _ => panic!("Expected BigUint variant"),
        }

        // Test large negative decimal string
        let result = UniversalNumber::from_string("-123456789012345678901234567890");
        assert!(result.is_ok());
        match result.unwrap() {
            UniversalNumber::BigInt(value) => {
                assert_eq!(value, I256::from_str("-123456789012345678901234567890").unwrap());
            }
            _ => panic!("Expected BigInt variant"),
        }

        // Test string with whitespace
        let result = UniversalNumber::from_string("  42  ");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), UniversalNumber::Small(42));

        // Test empty string
        let result = UniversalNumber::from_string("");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BigNumberError::ParseError { .. }));

        // Test whitespace only string
        let result = UniversalNumber::from_string("   ");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BigNumberError::ParseError { .. }));

        // Test invalid string
        let result = UniversalNumber::from_string("not_a_number");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BigNumberError::ParseError { .. }));

        // Test string with invalid characters
        let result = UniversalNumber::from_string("123abc");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BigNumberError::ParseError { .. }));
    }
}

