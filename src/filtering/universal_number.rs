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

