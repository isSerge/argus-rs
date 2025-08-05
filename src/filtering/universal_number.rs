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
#[derive(Debug, Clone)]
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

    /// Add two UniversalNumbers together
    pub fn add(&self, other: &Self) -> Result<Self, BigNumberError> {
        use UniversalNumber::*;
        
        match (self, other) {
            // Both Small - try to keep result small if possible
            (Small(a), Small(b)) => {
                match a.checked_add(*b) {
                    Some(result) => Ok(Small(result)),
                    None => {
                        // Overflow occurred, promote to big numbers
                        if *a > 0 && *b > 0 {
                            // Both positive, use BigUint
                            let a_big = U256::from(*a as u64);
                            let b_big = U256::from(*b as u64);
                            Ok(BigUint(a_big + b_big))
                        } else {
                            // At least one negative, use BigInt
                            let a_big = I256::try_from(*a).expect("i64 should fit in I256");
                            let b_big = I256::try_from(*b).expect("i64 should fit in I256");
                            Ok(Self::from_i256(a_big + b_big))
                        }
                    }
                }
            }
            
            // Small + BigUint
            (Small(a), BigUint(b)) => {
                if *a < 0 {
                    // Negative Small + positive BigUint: convert to BigInt arithmetic
                    let a_big = I256::try_from(*a).expect("i64 should fit in I256");
                    let b_big = I256::try_from(*b).map_err(|_| 
                        BigNumberError::ArithmeticOverflow { 
                            operation: "addition".to_string(), 
                            details: "BigUint too large to convert to I256".to_string() 
                        })?;
                    Ok(Self::from_i256(a_big + b_big))
                } else {
                    // Positive Small + BigUint: use BigUint arithmetic
                    let a_big = U256::from(*a as u64);
                    Ok(Self::from_u256(a_big + *b))
                }
            }
            (BigUint(a), Small(b)) => {
                // Symmetric case
                UniversalNumber::Small(*b).add(&UniversalNumber::BigUint(*a))
            }
            
            // Small + BigInt
            (Small(a), BigInt(b)) => {
                let a_big = I256::try_from(*a).expect("i64 should fit in I256");
                Ok(Self::from_i256(a_big + *b))
            }
            (BigInt(a), Small(b)) => {
                let b_big = I256::try_from(*b).expect("i64 should fit in I256");
                Ok(Self::from_i256(*a + b_big))
            }
            
            // BigUint + BigInt
            (BigUint(a), BigInt(b)) => {
                if b.is_negative() {
                    // BigUint + negative BigInt: convert BigUint to I256 and add
                    let a_big = I256::try_from(*a).map_err(|_|
                        BigNumberError::ArithmeticOverflow { 
                            operation: "addition".to_string(), 
                            details: "BigUint too large to convert to I256".to_string() 
                        })?;
                    Ok(Self::from_i256(a_big + *b))
                } else {
                    // BigUint + positive BigInt: convert BigInt to U256 and add
                    let b_big = U256::try_from(*b).map_err(|_|
                        BigNumberError::ArithmeticOverflow { 
                            operation: "addition".to_string(), 
                            details: "BigInt too large to convert to U256".to_string() 
                        })?;
                    Ok(Self::from_u256(*a + b_big))
                }
            }
            (BigInt(a), BigUint(b)) => {
                // Symmetric case
                UniversalNumber::BigUint(*b).add(&UniversalNumber::BigInt(*a))
            }
            
            // Both BigUint
            (BigUint(a), BigUint(b)) => {
                Ok(Self::from_u256(*a + *b))
            }
            
            // Both BigInt
            (BigInt(a), BigInt(b)) => {
                Ok(Self::from_i256(*a + *b))
            }
        }
    }

    /// Subtract other from self
    pub fn sub(&self, other: &Self) -> Result<Self, BigNumberError> {
        use UniversalNumber::*;
        
        match (self, other) {
            // Both Small - try to keep result small if possible
            (Small(a), Small(b)) => {
                match a.checked_sub(*b) {
                    Some(result) => Ok(Small(result)),
                    None => {
                        // Overflow occurred, promote to big numbers
                        let a_big = I256::try_from(*a).expect("i64 should fit in I256");
                        let b_big = I256::try_from(*b).expect("i64 should fit in I256");
                        Ok(Self::from_i256(a_big - b_big))
                    }
                }
            }
            
            // Small - BigUint
            (Small(a), BigUint(b)) => {
                // Always results in BigInt since Small - BigUint is likely negative
                let a_big = I256::try_from(*a).expect("i64 should fit in I256");
                let b_big = I256::try_from(*b).map_err(|_|
                    BigNumberError::ArithmeticOverflow { 
                        operation: "subtraction".to_string(), 
                        details: "BigUint too large to convert to I256".to_string() 
                    })?;
                Ok(Self::from_i256(a_big - b_big))
            }
            (BigUint(a), Small(b)) => {
                if *b < 0 {
                    // BigUint - negative Small = BigUint + positive Small
                    let b_pos = U256::from((-*b) as u64);
                    Ok(Self::from_u256(*a + b_pos))
                } else {
                    // BigUint - positive Small
                    let b_big = U256::from(*b as u64);
                    if *a >= b_big {
                        Ok(Self::from_u256(*a - b_big))
                    } else {
                        // Result would be negative, use BigInt
                        let a_big = I256::try_from(*a).map_err(|_|
                            BigNumberError::ArithmeticOverflow { 
                                operation: "subtraction".to_string(), 
                                details: "BigUint too large to convert to I256".to_string() 
                            })?;
                        let b_big = I256::try_from(*b).expect("i64 should fit in I256");
                        Ok(Self::from_i256(a_big - b_big))
                    }
                }
            }
            
            // Small - BigInt
            (Small(a), BigInt(b)) => {
                let a_big = I256::try_from(*a).expect("i64 should fit in I256");
                Ok(Self::from_i256(a_big - *b))
            }
            (BigInt(a), Small(b)) => {
                let b_big = I256::try_from(*b).expect("i64 should fit in I256");
                Ok(Self::from_i256(*a - b_big))
            }
            
            // BigUint - BigInt
            (BigUint(a), BigInt(b)) => {
                if b.is_negative() {
                    // BigUint - negative BigInt = BigUint + positive BigInt
                    let b_pos = -*b; // Make positive
                    let b_u256 = U256::try_from(b_pos).map_err(|_|
                        BigNumberError::ArithmeticOverflow { 
                            operation: "subtraction".to_string(), 
                            details: "BigInt too large to convert to U256".to_string() 
                        })?;
                    Ok(Self::from_u256(*a + b_u256))
                } else {
                    // BigUint - positive BigInt
                    let a_big = I256::try_from(*a).map_err(|_|
                        BigNumberError::ArithmeticOverflow { 
                            operation: "subtraction".to_string(), 
                            details: "BigUint too large to convert to I256".to_string() 
                        })?;
                    Ok(Self::from_i256(a_big - *b))
                }
            }
            (BigInt(a), BigUint(b)) => {
                // BigInt - BigUint: convert BigUint to I256 and subtract
                let b_big = I256::try_from(*b).map_err(|_|
                    BigNumberError::ArithmeticOverflow { 
                        operation: "subtraction".to_string(), 
                        details: "BigUint too large to convert to I256".to_string() 
                    })?;
                Ok(Self::from_i256(*a - b_big))
            }
            
            // Both BigUint
            (BigUint(a), BigUint(b)) => {
                if *a >= *b {
                    Ok(Self::from_u256(*a - *b))
                } else {
                    // Result would be negative, use BigInt
                    let a_big = I256::try_from(*a).expect("BigUint should convert to I256");
                    let b_big = I256::try_from(*b).expect("BigUint should convert to I256");
                    Ok(Self::from_i256(a_big - b_big))
                }
            }
            
            // Both BigInt
            (BigInt(a), BigInt(b)) => {
                Ok(Self::from_i256(*a - *b))
            }
        }
    }

    /// Multiply two UniversalNumbers
    pub fn mul(&self, other: &Self) -> Result<Self, BigNumberError> {
        use UniversalNumber::*;
        
        match (self, other) {
            // Both Small - try to keep result small if possible
            (Small(a), Small(b)) => {
                match a.checked_mul(*b) {
                    Some(result) => Ok(Small(result)),
                    None => {
                        // Overflow occurred, promote to big numbers
                        if (*a > 0 && *b > 0) || (*a < 0 && *b < 0) {
                            // Result is positive, try BigUint first
                            let a_abs = a.abs() as u64;
                            let b_abs = b.abs() as u64;
                            let result = U256::from(a_abs) * U256::from(b_abs);
                            Ok(Self::from_u256(result))
                        } else {
                            // Result is negative, use BigInt
                            let a_big = I256::try_from(*a).expect("i64 should fit in I256");
                            let b_big = I256::try_from(*b).expect("i64 should fit in I256");
                            Ok(Self::from_i256(a_big * b_big))
                        }
                    }
                }
            }
            
            // Small * BigUint
            (Small(a), BigUint(b)) => {
                if *a == 0 || b.is_zero() {
                    Ok(Small(0))
                } else if *a > 0 {
                    // Positive Small * BigUint = BigUint
                    let a_big = U256::from(*a as u64);
                    Ok(Self::from_u256(a_big * *b))
                } else {
                    // Negative Small * BigUint = negative BigInt
                    let a_big = I256::try_from(*a).expect("i64 should fit in I256");
                    let b_big = I256::try_from(*b).map_err(|_|
                        BigNumberError::ArithmeticOverflow { 
                            operation: "multiplication".to_string(), 
                            details: "BigUint too large to convert to I256".to_string() 
                        })?;
                    Ok(Self::from_i256(a_big * b_big))
                }
            }
            (BigUint(a), Small(b)) => {
                // Symmetric case
                UniversalNumber::Small(*b).mul(&UniversalNumber::BigUint(*a))
            }
            
            // Small * BigInt
            (Small(a), BigInt(b)) => {
                if *a == 0 || b.is_zero() {
                    Ok(Small(0))
                } else {
                    let a_big = I256::try_from(*a).expect("i64 should fit in I256");
                    Ok(Self::from_i256(a_big * *b))
                }
            }
            (BigInt(a), Small(b)) => {
                if a.is_zero() || *b == 0 {
                    Ok(Small(0))
                } else {
                    let b_big = I256::try_from(*b).expect("i64 should fit in I256");
                    Ok(Self::from_i256(*a * b_big))
                }
            }
            
            // BigUint * BigInt
            (BigUint(a), BigInt(b)) => {
                if a.is_zero() || b.is_zero() {
                    Ok(Small(0))
                } else if b.is_positive() {
                    // BigUint * positive BigInt: try to keep as BigUint
                    let b_u256 = U256::try_from(*b).map_err(|_|
                        BigNumberError::ArithmeticOverflow { 
                            operation: "multiplication".to_string(), 
                            details: "BigInt too large to convert to U256".to_string() 
                        })?;
                    Ok(Self::from_u256(*a * b_u256))
                } else {
                    // BigUint * negative BigInt: result is negative BigInt
                    let a_big = I256::try_from(*a).map_err(|_|
                        BigNumberError::ArithmeticOverflow { 
                            operation: "multiplication".to_string(), 
                            details: "BigUint too large to convert to I256".to_string() 
                        })?;
                    Ok(Self::from_i256(a_big * *b))
                }
            }
            (BigInt(a), BigUint(b)) => {
                // Symmetric case
                UniversalNumber::BigUint(*b).mul(&UniversalNumber::BigInt(*a))
            }
            
            // Both BigUint
            (BigUint(a), BigUint(b)) => {
                if a.is_zero() || b.is_zero() {
                    Ok(Small(0))
                } else {
                    Ok(Self::from_u256(*a * *b))
                }
            }
            
            // Both BigInt
            (BigInt(a), BigInt(b)) => {
                if a.is_zero() || b.is_zero() {
                    Ok(Small(0))
                } else {
                    Ok(Self::from_i256(*a * *b))
                }
            }
        }
    }

    /// Divide self by other
    pub fn div(&self, other: &Self) -> Result<Self, BigNumberError> {
        // Check for division by zero
        if other.is_zero() {
            return Err(BigNumberError::DivisionByZero);
        }
        
        use UniversalNumber::*;
        
        match (self, other) {
            // Both Small
            (Small(a), Small(b)) => {
                // Integer division
                Ok(Small(*a / *b))
            }
            
            // Small / BigUint
            (Small(a), BigUint(_)) => {
                // Small number divided by large positive number is always 0 or very small
                // For integer division, this is 0 unless the small number is negative
                if *a < 0 {
                    Ok(Small(-1)) // -1 for negative numbers divided by large positive
                } else {
                    Ok(Small(0))
                }
            }
            (BigUint(a), Small(b)) => {
                if *b > 0 {
                    let b_big = U256::from(*b as u64);
                    Ok(Self::from_u256(*a / b_big))
                } else {
                    // BigUint / negative Small: convert to BigInt division
                    let a_big = I256::try_from(*a).map_err(|_|
                        BigNumberError::ArithmeticOverflow { 
                            operation: "division".to_string(), 
                            details: "BigUint too large to convert to I256".to_string() 
                        })?;
                    let b_big = I256::try_from(*b).expect("i64 should fit in I256");
                    Ok(Self::from_i256(a_big / b_big))
                }
            }
            
            // Small / BigInt
            (Small(a), BigInt(b)) => {
                // Small number divided by large number
                if b.is_positive() {
                    if *a < 0 {
                        Ok(Small(-1))
                    } else {
                        Ok(Small(0))
                    }
                } else {
                    // Dividing by negative large number
                    if *a > 0 {
                        Ok(Small(-1))
                    } else {
                        Ok(Small(0))
                    }
                }
            }
            (BigInt(a), Small(b)) => {
                let b_big = I256::try_from(*b).expect("i64 should fit in I256");
                Ok(Self::from_i256(*a / b_big))
            }
            
            // BigUint / BigInt
            (BigUint(a), BigInt(b)) => {
                if b.is_positive() {
                    let b_u256 = U256::try_from(*b).map_err(|_|
                        BigNumberError::ArithmeticOverflow { 
                            operation: "division".to_string(), 
                            details: "BigInt too large to convert to U256".to_string() 
                        })?;
                    Ok(Self::from_u256(*a / b_u256))
                } else {
                    // BigUint / negative BigInt: result is negative
                    let a_big = I256::try_from(*a).map_err(|_|
                        BigNumberError::ArithmeticOverflow { 
                            operation: "division".to_string(), 
                            details: "BigUint too large to convert to I256".to_string() 
                        })?;
                    Ok(Self::from_i256(a_big / *b))
                }
            }
            (BigInt(a), BigUint(b)) => {
                let b_big = I256::try_from(*b).map_err(|_|
                    BigNumberError::ArithmeticOverflow { 
                        operation: "division".to_string(), 
                        details: "BigUint too large to convert to I256".to_string() 
                    })?;
                Ok(Self::from_i256(*a / b_big))
            }
            
            // Both BigUint
            (BigUint(a), BigUint(b)) => {
                Ok(Self::from_u256(*a / *b))
            }
            
            // Both BigInt
            (BigInt(a), BigInt(b)) => {
                Ok(Self::from_i256(*a / *b))
            }
        }
    }
}

impl PartialOrd for UniversalNumber {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for UniversalNumber {
    fn cmp(&self, other: &Self) -> Ordering {
        use UniversalNumber::*;
        
        match (self, other) {
            // Both are Small - direct comparison
            (Small(a), Small(b)) => a.cmp(b),
            
            // Small vs BigUint
            (Small(a), BigUint(b)) => {
                if *a < 0 {
                    // Negative Small is always less than positive BigUint
                    Ordering::Less
                } else {
                    // Compare positive Small with BigUint
                    U256::from(*a as u64).cmp(b)
                }
            }
            (BigUint(a), Small(b)) => {
                if *b < 0 {
                    // Positive BigUint is always greater than negative Small
                    Ordering::Greater
                } else {
                    // Compare BigUint with positive Small
                    a.cmp(&U256::from(*b as u64))
                }
            }
            
            // Small vs BigInt
            (Small(a), BigInt(b)) => {
                // Convert Small to I256 for comparison
                let a_i256 = I256::try_from(*a).expect("i64 should always fit in I256");
                a_i256.cmp(b)
            }
            (BigInt(a), Small(b)) => {
                // Convert Small to I256 for comparison
                let b_i256 = I256::try_from(*b).expect("i64 should always fit in I256");
                a.cmp(&b_i256)
            }
            
            // BigUint vs BigInt
            (BigUint(a), BigInt(b)) => {
                if b.is_negative() {
                    // Positive BigUint is always greater than negative BigInt
                    Ordering::Greater
                } else {
                    // Both are non-negative, convert BigUint to I256 for comparison
                    // Since BigUint can be larger than I256::MAX, we need to handle overflow
                    if *a > U256::from(I256::MAX) {
                        // BigUint is larger than max I256
                        Ordering::Greater
                    } else {
                        // Safe to convert BigUint to I256
                        let a_i256 = I256::try_from(*a).expect("BigUint should fit in I256 when <= I256::MAX");
                        a_i256.cmp(b)
                    }
                }
            }
            (BigInt(a), BigUint(b)) => {
                if a.is_negative() {
                    // Negative BigInt is always less than positive BigUint
                    Ordering::Less
                } else {
                    // Both are non-negative, convert BigUint to I256 for comparison
                    // Since BigUint can be larger than I256::MAX, we need to handle overflow
                    if *b > U256::from(I256::MAX) {
                        // BigUint is larger than max I256
                        Ordering::Less
                    } else {
                        // Safe to convert BigUint to I256
                        let b_i256 = I256::try_from(*b).expect("BigUint should fit in I256 when <= I256::MAX");
                        a.cmp(&b_i256)
                    }
                }
            }
            
            // Both are BigUint
            (BigUint(a), BigUint(b)) => a.cmp(b),
            
            // Both are BigInt
            (BigInt(a), BigInt(b)) => a.cmp(b),
        }
    }
}

impl PartialEq for UniversalNumber {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for UniversalNumber {}

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

    #[test]
    fn test_from_dynamic() {
        // Test Rhai INT (i64) values
        let rhai_int = rhai::Dynamic::from(42i64);
        let result = UniversalNumber::from_dynamic(&rhai_int);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), UniversalNumber::Small(42));

        // Test negative Rhai INT
        let rhai_negative = rhai::Dynamic::from(-123i64);
        let result = UniversalNumber::from_dynamic(&rhai_negative);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), UniversalNumber::Small(-123));

        // Test zero Rhai INT
        let rhai_zero = rhai::Dynamic::from(0i64);
        let result = UniversalNumber::from_dynamic(&rhai_zero);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), UniversalNumber::Small(0));

        // Test Rhai INT at boundaries
        let rhai_max = rhai::Dynamic::from(i64::MAX);
        let result = UniversalNumber::from_dynamic(&rhai_max);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), UniversalNumber::Small(i64::MAX));

        let rhai_min = rhai::Dynamic::from(i64::MIN);
        let result = UniversalNumber::from_dynamic(&rhai_min);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), UniversalNumber::Small(i64::MIN));

        // Test String that fits in i64
        let string_small = rhai::Dynamic::from("42".to_string());
        let result = UniversalNumber::from_dynamic(&string_small);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), UniversalNumber::Small(42));

        // Test String with negative number
        let string_negative = rhai::Dynamic::from("-123".to_string());
        let result = UniversalNumber::from_dynamic(&string_negative);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), UniversalNumber::Small(-123));

        // Test String with large positive number
        let string_large_pos = rhai::Dynamic::from("123456789012345678901234567890".to_string());
        let result = UniversalNumber::from_dynamic(&string_large_pos);
        assert!(result.is_ok());
        match result.unwrap() {
            UniversalNumber::BigUint(value) => {
                assert_eq!(value, U256::from_str("123456789012345678901234567890").unwrap());
            }
            _ => panic!("Expected BigUint variant"),
        }

        // Test String with large negative number
        let string_large_neg = rhai::Dynamic::from("-123456789012345678901234567890".to_string());
        let result = UniversalNumber::from_dynamic(&string_large_neg);
        assert!(result.is_ok());
        match result.unwrap() {
            UniversalNumber::BigInt(value) => {
                assert_eq!(value, I256::from_str("-123456789012345678901234567890").unwrap());
            }
            _ => panic!("Expected BigInt variant"),
        }

        // Test String with whitespace
        let string_whitespace = rhai::Dynamic::from("  42  ".to_string());
        let result = UniversalNumber::from_dynamic(&string_whitespace);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), UniversalNumber::Small(42));

        // Test invalid String
        let invalid_string = rhai::Dynamic::from("not_a_number".to_string());
        let result = UniversalNumber::from_dynamic(&invalid_string);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BigNumberError::ParseError { .. }));

        // Test empty String
        let empty_string = rhai::Dynamic::from("".to_string());
        let result = UniversalNumber::from_dynamic(&empty_string);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BigNumberError::ParseError { .. }));

        // Test boolean (should convert to string and fail parsing)
        let rhai_bool = rhai::Dynamic::from(true);
        let result = UniversalNumber::from_dynamic(&rhai_bool);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BigNumberError::ParseError { .. }));

        // Test float (should convert to string)
        let rhai_float = rhai::Dynamic::from(42.5f64);
        let result = UniversalNumber::from_dynamic(&rhai_float);
        // This should fail since we don't handle decimal numbers
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BigNumberError::ParseError { .. }));
    }

    #[test]
    fn test_comparison_small_numbers() {
        let a = UniversalNumber::Small(42);
        let b = UniversalNumber::Small(100);
        let c = UniversalNumber::Small(42);
        let d = UniversalNumber::Small(-10);

        // Test ordering
        assert!(a < b);
        assert!(b > a);
        assert!(a == c);
        assert!(a >= c);
        assert!(a <= c);
        assert!(d < a);
        assert!(a > d);

        // Test with zero
        let zero = UniversalNumber::Small(0);
        assert!(d < zero);
        assert!(zero < a);
        assert!(zero > d);
    }

    #[test]
    fn test_comparison_small_vs_biguint() {
        let small_pos = UniversalNumber::Small(42);
        let small_neg = UniversalNumber::Small(-42);
        let small_zero = UniversalNumber::Small(0);
        let big_uint = UniversalNumber::BigUint(U256::from(100));
        let big_uint_large = UniversalNumber::BigUint(U256::from_str("123456789012345678901234567890").unwrap());

        // Positive small vs BigUint
        assert!(small_pos < big_uint);
        assert!(big_uint > small_pos);
        assert!(small_pos < big_uint_large);

        // Negative small vs BigUint (negative always less than positive BigUint)
        assert!(small_neg < big_uint);
        assert!(big_uint > small_neg);
        assert!(small_neg < big_uint_large);

        // Zero vs BigUint
        let big_uint_zero = UniversalNumber::BigUint(U256::ZERO);
        assert!(small_zero == big_uint_zero);
        assert!(small_zero < big_uint);
    }

    #[test]
    fn test_comparison_small_vs_bigint() {
        let small_pos = UniversalNumber::Small(42);
        let small_neg = UniversalNumber::Small(-42);
        let big_int_pos = UniversalNumber::BigInt(I256::from_str("123456789012345678901234567890").unwrap());
        let big_int_neg = UniversalNumber::BigInt(I256::from_str("-123456789012345678901234567890").unwrap());

        // Small positive vs BigInt positive
        assert!(small_pos < big_int_pos);
        assert!(big_int_pos > small_pos);

        // Small negative vs BigInt negative (smaller magnitude)
        assert!(small_neg > big_int_neg);
        assert!(big_int_neg < small_neg);

        // Small positive vs BigInt negative
        assert!(small_pos > big_int_neg);
        assert!(big_int_neg < small_pos);

        // Small negative vs BigInt positive
        assert!(small_neg < big_int_pos);
        assert!(big_int_pos > small_neg);
    }

    #[test]
    fn test_comparison_biguint_vs_bigint() {
        let big_uint = UniversalNumber::BigUint(U256::from_str("123456789012345678901234567890").unwrap());
        let big_uint_zero = UniversalNumber::BigUint(U256::ZERO);
        let big_int_pos = UniversalNumber::BigInt(I256::from_str("123456789012345678901234567890").unwrap());
        let big_int_neg = UniversalNumber::BigInt(I256::from_str("-123456789012345678901234567890").unwrap());
        let big_int_zero = UniversalNumber::BigInt(I256::ZERO);

        // BigUint vs negative BigInt
        assert!(big_uint > big_int_neg);
        assert!(big_int_neg < big_uint);

        // BigUint vs positive BigInt (same value)
        assert!(big_uint == big_int_pos);
        assert!(big_int_pos == big_uint);

        // Zero comparisons
        assert!(big_uint_zero == big_int_zero);
        assert!(big_uint > big_int_neg);
        assert!(big_uint_zero > big_int_neg);
    }

    #[test]
    fn test_comparison_same_variants() {
        // BigUint vs BigUint
        let big_uint_1 = UniversalNumber::BigUint(U256::from(100));
        let big_uint_2 = UniversalNumber::BigUint(U256::from(200));
        assert!(big_uint_1 < big_uint_2);
        assert!(big_uint_2 > big_uint_1);

        // BigInt vs BigInt
        let big_int_1 = UniversalNumber::BigInt(I256::from_str("100").unwrap());
        let big_int_2 = UniversalNumber::BigInt(I256::from_str("200").unwrap());
        let big_int_neg = UniversalNumber::BigInt(I256::from_str("-100").unwrap());
        assert!(big_int_1 < big_int_2);
        assert!(big_int_2 > big_int_1);
        assert!(big_int_neg < big_int_1);
        assert!(big_int_1 > big_int_neg);
    }

    #[test]
    fn test_comparison_edge_cases() {
        // i64::MAX boundary cases
        let small_max = UniversalNumber::Small(i64::MAX);
        let big_uint_just_above = UniversalNumber::BigUint(U256::from((i64::MAX as u64) + 1));
        assert!(small_max < big_uint_just_above);

        // Large BigUint vs I256::MAX boundary
        let very_large_biguint = UniversalNumber::BigUint(U256::from_str("99999999999999999999999999999999999999999999999999999999999999999999999999999").unwrap());
        let i256_max_bigint = UniversalNumber::BigInt(I256::MAX);
        assert!(very_large_biguint > i256_max_bigint);

        // Zero across all variants
        let small_zero = UniversalNumber::Small(0);
        let biguint_zero = UniversalNumber::BigUint(U256::ZERO);
        let bigint_zero = UniversalNumber::BigInt(I256::ZERO);
        assert!(small_zero == biguint_zero);
        assert!(small_zero == bigint_zero);
        assert!(biguint_zero == bigint_zero);
    }

    #[test]
    fn test_arithmetic_operations() {
        // Test addition
        let a = UniversalNumber::from_i64(42);
        let b = UniversalNumber::from_i64(58);
        let result = a.add(&b).unwrap();
        assert_eq!(result, UniversalNumber::from_i64(100));

        // Test subtraction
        let a = UniversalNumber::from_i64(100);
        let b = UniversalNumber::from_i64(42);
        let result = a.sub(&b).unwrap();
        assert_eq!(result, UniversalNumber::from_i64(58));

        // Test multiplication
        let a = UniversalNumber::from_i64(6);
        let b = UniversalNumber::from_i64(7);
        let result = a.mul(&b).unwrap();
        assert_eq!(result, UniversalNumber::from_i64(42));

        // Test division
        let a = UniversalNumber::from_i64(42);
        let b = UniversalNumber::from_i64(2);
        let result = a.div(&b).unwrap();
        assert_eq!(result, UniversalNumber::from_i64(21));

        // Test division by zero
        let a = UniversalNumber::from_i64(42);
        let b = UniversalNumber::from_i64(0);
        let result = a.div(&b);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BigNumberError::DivisionByZero));

        // Test addition overflow - should promote to BigUint
        let a = UniversalNumber::from_i64(i64::MAX);
        let b = UniversalNumber::from_i64(1);
        let result = a.add(&b).unwrap();
        match result {
            UniversalNumber::BigUint(value) => {
                assert_eq!(value, U256::from(i64::MAX as u64) + U256::from(1));
            }
            _ => panic!("Expected BigUint for overflow case"),
        }

        // Test subtraction overflow - should promote to BigInt
        let a = UniversalNumber::from_i64(i64::MIN);
        let b = UniversalNumber::from_i64(1);
        let result = a.sub(&b).unwrap();
        match result {
            UniversalNumber::BigInt(value) => {
                let expected = I256::try_from(i64::MIN).unwrap() - I256::ONE;
                assert_eq!(value, expected);
            }
            _ => panic!("Expected BigInt for underflow case"),
        }

        // Test multiplication overflow - should promote to BigUint
        let a = UniversalNumber::from_i64(30000);
        let b = UniversalNumber::from_i64(30000);
        let result = a.mul(&b).unwrap();
        match result {
            UniversalNumber::BigUint(_) => {}, // Expected promotion to BigUint
            UniversalNumber::Small(val) => {
                // If it fits in Small, that's also valid
                assert_eq!(val, 30000i64 * 30000i64);
            }
            _ => panic!("Expected BigUint or Small for multiplication"),
        }

        // Test division resulting in non-integer (should truncate towards zero)
        let a = UniversalNumber::from_i64(7);
        let b = UniversalNumber::from_i64(3);
        let result = a.div(&b).unwrap();
        assert_eq!(result, UniversalNumber::from_i64(2));
    }

    #[test]
    fn test_arithmetic_small_numbers() {
        let a = UniversalNumber::Small(10);
        let b = UniversalNumber::Small(5);
        let c = UniversalNumber::Small(-3);

        // Addition
        let result = a.add(&b).unwrap();
        assert_eq!(result, UniversalNumber::Small(15));

        let result = a.add(&c).unwrap();
        assert_eq!(result, UniversalNumber::Small(7));

        let result = c.add(&c).unwrap();
        assert_eq!(result, UniversalNumber::Small(-6));

        // Subtraction
        let result = a.sub(&b).unwrap();
        assert_eq!(result, UniversalNumber::Small(5));

        let result = b.sub(&a).unwrap();
        assert_eq!(result, UniversalNumber::Small(-5));

        let result = a.sub(&c).unwrap();
        assert_eq!(result, UniversalNumber::Small(13));

        // Multiplication
        let result = a.mul(&b).unwrap();
        assert_eq!(result, UniversalNumber::Small(50));

        let result = a.mul(&c).unwrap();
        assert_eq!(result, UniversalNumber::Small(-30));

        let result = c.mul(&c).unwrap();
        assert_eq!(result, UniversalNumber::Small(9));

        // Division
        let result = a.div(&b).unwrap();
        assert_eq!(result, UniversalNumber::Small(2));

        let result = a.div(&c).unwrap();
        assert_eq!(result, UniversalNumber::Small(-3));

        let result = c.div(&b).unwrap();
        assert_eq!(result, UniversalNumber::Small(0)); // Integer division: -3/5 = 0
    }

    #[test]
    fn test_arithmetic_overflow_handling() {
        // Test addition overflow
        let large_pos = UniversalNumber::Small(i64::MAX);
        let one = UniversalNumber::Small(1);
        let result = large_pos.add(&one).unwrap();
        // Should promote to BigUint
        match result {
            UniversalNumber::BigUint(value) => {
                assert_eq!(value, U256::from(i64::MAX as u64) + U256::from(1));
            }
            _ => panic!("Expected BigUint for overflow case"),
        }

        // Test subtraction underflow
        let large_neg = UniversalNumber::Small(i64::MIN);
        let result = large_neg.sub(&one).unwrap();
        // Should promote to BigInt
        match result {
            UniversalNumber::BigInt(value) => {
                let expected = I256::try_from(i64::MIN).unwrap() - I256::ONE;
                assert_eq!(value, expected);
            }
            _ => panic!("Expected BigInt for underflow case"),
        }

        // Test multiplication overflow
        let large = UniversalNumber::Small(i64::MAX / 2 + 1);
        let result = large.mul(&UniversalNumber::Small(2)).unwrap();
        // Should promote to BigUint
        match result {
            UniversalNumber::BigUint(_) => {} // Expected
            _ => panic!("Expected BigUint for multiplication overflow"),
        }
    }

    #[test]
    fn test_arithmetic_with_zero() {
        let zero = UniversalNumber::Small(0);
        let big_uint = UniversalNumber::BigUint(U256::from_str("123456789012345678901234567890").unwrap());
        let big_int = UniversalNumber::BigInt(I256::from_str("-123456789012345678901234567890").unwrap());

        // Addition with zero
        assert_eq!(zero.add(&big_uint).unwrap(), big_uint);
        assert_eq!(big_uint.add(&zero).unwrap(), big_uint);
        assert_eq!(zero.add(&big_int).unwrap(), big_int);

        // Subtraction with zero
        assert_eq!(big_uint.sub(&zero).unwrap(), big_uint);
        
        // zero - big_uint should be negative
        let result = zero.sub(&big_uint).unwrap();
        match result {
            UniversalNumber::BigInt(value) => {
                assert!(value.is_negative());
            }
            _ => panic!("Expected negative BigInt"),
        }

        // Multiplication with zero
        assert_eq!(zero.mul(&big_uint).unwrap(), UniversalNumber::Small(0));
        assert_eq!(big_uint.mul(&zero).unwrap(), UniversalNumber::Small(0));
        assert_eq!(zero.mul(&big_int).unwrap(), UniversalNumber::Small(0));

        // Division by zero should error
        assert!(big_uint.div(&zero).is_err());
        assert!(matches!(big_uint.div(&zero).unwrap_err(), BigNumberError::DivisionByZero));
    }

    #[test]
    fn test_arithmetic_mixed_variants() {
        let small = UniversalNumber::Small(42);
        let big_uint = UniversalNumber::BigUint(U256::from(1000));
        let big_int = UniversalNumber::BigInt(I256::from_str("123456789012345678901234567890").unwrap());

        // Small + BigUint
        let result = small.add(&big_uint).unwrap();
        assert_eq!(result, UniversalNumber::BigUint(U256::from(1042)));

        // Small - BigUint (should be negative but might fit in Small)
        let result = small.sub(&big_uint).unwrap();
        assert!(result.is_negative()); // Should be negative regardless of variant
        
        // Small * BigInt (large result)
        let result = small.mul(&big_int).unwrap();
        match result {
            UniversalNumber::BigInt(value) => {
                // Check that it's a big positive number (42 * large_positive)
                assert!(value.is_positive());
            }
            _ => panic!("Expected BigInt"),
        }

        // BigUint / Small
        let result = big_uint.div(&small).unwrap();
        assert_eq!(result, UniversalNumber::Small(23)); // 1000 / 42 = 23 (integer division)
    }

    #[test]
    fn test_arithmetic_big_numbers() {
        let big_uint_1 = UniversalNumber::BigUint(U256::from_str("123456789012345678901234567890").unwrap());
        let big_uint_2 = UniversalNumber::BigUint(U256::from_str("987654321098765432109876543210").unwrap());
        let big_int_pos = UniversalNumber::BigInt(I256::from_str("123456789012345678901234567890").unwrap());
        let big_int_neg = UniversalNumber::BigInt(I256::from_str("-123456789012345678901234567890").unwrap());

        // BigUint + BigUint
        let result = big_uint_1.add(&big_uint_2).unwrap();
        match result {
            UniversalNumber::BigUint(value) => {
                let expected = U256::from_str("123456789012345678901234567890").unwrap() +
                              U256::from_str("987654321098765432109876543210").unwrap();
                assert_eq!(value, expected);
            }
            _ => panic!("Expected BigUint"),
        }

        // BigUint - BigUint (larger - smaller)
        let result = big_uint_2.sub(&big_uint_1).unwrap();
        match result {
            UniversalNumber::BigUint(value) => {
                let expected = U256::from_str("987654321098765432109876543210").unwrap() -
                              U256::from_str("123456789012345678901234567890").unwrap();
                assert_eq!(value, expected);
            }
            _ => panic!("Expected BigUint"),
        }

        // BigUint - BigUint (smaller - larger, should be negative)
        let result = big_uint_1.sub(&big_uint_2).unwrap();
        match result {
            UniversalNumber::BigInt(value) => {
                assert!(value.is_negative());
            }
            _ => panic!("Expected negative BigInt"),
        }

        // BigUint + negative BigInt
        let result = big_uint_1.add(&big_int_neg).unwrap();
        assert_eq!(result, UniversalNumber::Small(0)); // Same magnitude, opposite signs

        // BigInt * BigInt
        let small_big_int = UniversalNumber::BigInt(I256::try_from(100).unwrap());
        let result = big_int_pos.mul(&small_big_int).unwrap();
        match result {
            UniversalNumber::BigInt(value) => {
                // Should be big_int_pos * 100, which is a very large positive number
                assert!(value.is_positive());
            }
            _ => panic!("Expected BigInt"),
        }
    }

    #[test]
    fn test_division_edge_cases() {
        let big_uint = UniversalNumber::BigUint(U256::from(1000));
        let small_pos = UniversalNumber::Small(7);
        let small_neg = UniversalNumber::Small(-7);

        // BigUint / positive Small
        let result = big_uint.div(&small_pos).unwrap();
        assert_eq!(result, UniversalNumber::Small(142)); // 1000 / 7 = 142 (integer division)

        // BigUint / negative Small
        let result = big_uint.div(&small_neg).unwrap();
        match result {
            UniversalNumber::Small(value) => {
                // For BigUint / negative Small, the result should be negative
                assert_eq!(value, -142); // 1000 / -7 = -142
            }
            UniversalNumber::BigInt(value) => {
                assert!(value.is_negative());
                let expected = I256::try_from(-142).unwrap();
                assert_eq!(value, expected);
            }
            _ => panic!("Expected negative result"),
        }

        // Small / BigUint (should be 0 for positive, -1 for negative)
        let result = small_pos.div(&big_uint).unwrap();
        assert_eq!(result, UniversalNumber::Small(0));

        let result = small_neg.div(&big_uint).unwrap();
        assert_eq!(result, UniversalNumber::Small(-1));

        // Division by zero
        let zero = UniversalNumber::Small(0);
        assert!(big_uint.div(&zero).is_err());
        assert!(matches!(big_uint.div(&zero).unwrap_err(), BigNumberError::DivisionByZero));
    }

    #[test]
    fn test_arithmetic_result_optimization() {
        // Test that results are kept in Small variant when possible
        let big_uint_small = UniversalNumber::BigUint(U256::from(100));
        let big_int_small = UniversalNumber::BigInt(I256::try_from(50).unwrap());

        // BigUint - BigInt where result fits in Small
        let result = big_uint_small.sub(&big_int_small).unwrap();
        assert_eq!(result, UniversalNumber::Small(50));

        // BigInt + Small where result fits in Small
        let small = UniversalNumber::Small(25);
        let result = big_int_small.add(&small).unwrap();
        assert_eq!(result, UniversalNumber::Small(75));

        // BigUint / BigUint where result fits in Small
        let big_uint_1000 = UniversalNumber::BigUint(U256::from(1000));
        let big_uint_10 = UniversalNumber::BigUint(U256::from(10));
        let result = big_uint_1000.div(&big_uint_10).unwrap();
        assert_eq!(result, UniversalNumber::Small(100));
    }

    #[test]
    fn test_arithmetic_operations_full() {
        let a = UniversalNumber::from_i64(42);
        let b = UniversalNumber::from_i64(-7);
        let c = UniversalNumber::BigUint(U256::from(100));
        let d = UniversalNumber::BigInt(I256::from_str("123456789012345678901234567890").unwrap());

        // Addition - all operations should succeed with mixed variant support
        assert_eq!(a.add(&a).unwrap(), UniversalNumber::from_i64(84));
        assert_eq!(a.add(&b).unwrap(), UniversalNumber::from_i64(35));
        assert_eq!(a.add(&c).unwrap(), UniversalNumber::Small(142)); // Optimized to Small since 42 + 100 = 142 fits in i64
        
        // a + d should succeed (Small + BigInt)
        let result_a_plus_d = a.add(&d).unwrap();
        match result_a_plus_d {
            UniversalNumber::BigInt(_) => {}, // Expected for large result
            _ => panic!("Expected BigInt for large addition"),
        }

        // Subtraction - all should succeed
        assert_eq!(a.sub(&a).unwrap(), UniversalNumber::from_i64(0));
        assert_eq!(a.sub(&b).unwrap(), UniversalNumber::from_i64(49));
        
        // a - c should result in negative value (42 - 100 = -58)
        let result_a_minus_c = a.sub(&c).unwrap();
        assert_eq!(result_a_minus_c, UniversalNumber::Small(-58));
        
        // a - d should succeed (Small - BigInt)
        let result_a_minus_d = a.sub(&d).unwrap();
        match result_a_minus_d {
            UniversalNumber::BigInt(value) => {
                assert!(value.is_negative()); // Should be very negative
            }
            _ => panic!("Expected negative BigInt"),
        }

        // Multiplication - all should succeed
        assert_eq!(a.mul(&a).unwrap(), UniversalNumber::from_i64(1764));
        assert_eq!(a.mul(&b).unwrap(), UniversalNumber::from_i64(-294));
        assert_eq!(a.mul(&c).unwrap(), UniversalNumber::Small(4200)); // 42 * 100 = 4200, fits in i64
        
        // a * d should succeed (Small * BigInt)
        let result_a_times_d = a.mul(&d).unwrap();
        match result_a_times_d {
            UniversalNumber::BigInt(_) => {}, // Expected for large result
            _ => panic!("Expected BigInt for large multiplication"),
        }

        // Division - all should succeed
        assert_eq!(a.div(&a).unwrap(), UniversalNumber::from_i64(1));
        assert_eq!(a.div(&b).unwrap(), UniversalNumber::Small(-6)); // Rounds towards zero
        assert_eq!(c.div(&a).unwrap(), UniversalNumber::Small(2)); // 100 / 42 = 2 (integer division)
        
        // c / d should succeed (BigUint / BigInt) and result in 0 (since 100 < large d)
        let result_c_div_d = c.div(&d).unwrap();
        assert_eq!(result_c_div_d, UniversalNumber::Small(0)); // Integer division: small/large = 0
    }
}

