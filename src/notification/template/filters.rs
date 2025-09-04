//! This module provides custom filters for the minijinja templating engine

use std::str::FromStr;

use minijinja::{
    Error, ErrorKind, State,
    value::{Value, ValueKind},
};
use rust_decimal::{Decimal, MathematicalOps};

/// A minijinja filter that sums a sequence of string-represented decimal
/// numbers.
pub fn sum(values: Value) -> Result<String, Error> {
    if values.kind() != ValueKind::Seq {
        return Err(Error::new(
            ErrorKind::InvalidOperation,
            "sum filter can only be applied to a sequence.",
        ));
    }

    let mut total = Decimal::ZERO;
    for value in values.try_iter()? {
        let s = value.to_string();
        let num = Decimal::from_str(&s).map_err(|e| {
            Error::new(
                ErrorKind::InvalidOperation,
                format!("Failed to parse value for sum: {s} ({e})"),
            )
        })?;
        total += num;
    }

    Ok(total.to_string())
}

/// A minijinja filter that averages a sequence of string-represented decimal
/// numbers.
pub fn avg(values: Value) -> Result<String, Error> {
    if values.kind() != ValueKind::Seq {
        return Err(Error::new(
            ErrorKind::InvalidOperation,
            "avg filter can only be applied to a sequence.",
        ));
    }

    let mut total = Decimal::ZERO;
    let mut count = 0;
    for value in values.try_iter()? {
        let s = value.to_string();
        let num = Decimal::from_str(&s).map_err(|e| {
            Error::new(
                ErrorKind::InvalidOperation,
                format!("Failed to parse value for avg: {s} ({e})"),
            )
        })?;
        total += num;
        count += 1;
    }

    if count == 0 {
        return Ok(Decimal::ZERO.to_string());
    }

    Ok((total / Decimal::from(count)).to_string())
}

/// A minijinja filter that formats a given amount by scaling it down by a
/// specified number of decimals.
///
/// This filter is useful for converting large integer values (like token
/// amounts in their smallest unit, e.g., wei for Ethereum) into a more
/// human-readable floating-point format.
///
/// # Arguments
///
/// * `value`: The amount to be formatted. This can be a string or a number.
/// * `decimals`: The number of decimal places to scale the value by.
///
/// # Example
///
/// ```jinja
/// {{ log.params.usdc_value | decimals(6) }}
/// ```
pub fn decimals(_state: &State, value: String, decimals: u32) -> Result<String, Error> {
    let amount = Decimal::from_str(&value).map_err(|e| {
        Error::new(ErrorKind::InvalidOperation, format!("Failed to parse amount: {e}"))
    })?;

    Ok(scale_by_decimals(amount, decimals))
}

/// A minijinja filter that formats a given amount in wei into ether.
///
/// # Arguments
///
/// * `value`: The amount in wei to be formatted. This can be a string or a
///   number.
///
/// # Example
///
/// ```jinja
/// {{ transaction.value | ether }}
/// ```
pub fn ether(_state: &State, value: String) -> Result<String, Error> {
    let amount = Decimal::from_str(&value).map_err(|e| {
        Error::new(ErrorKind::InvalidOperation, format!("Failed to parse amount: {e}"))
    })?;

    Ok(scale_by_decimals(amount, 18))
}

/// A minijinja filter that formats a given amount in wei into gwei.
///
/// # Arguments
///
/// * `value`: The amount in wei to be formatted. This can be a string or a
///   number.
///
/// # Example
///
/// ```jinja
/// {{ transaction.gas_price | gwei }}
/// ```
pub fn gwei(_state: &State, value: String) -> Result<String, Error> {
    let amount = Decimal::from_str(&value).map_err(|e| {
        Error::new(ErrorKind::InvalidOperation, format!("Failed to parse amount: {e}"))
    })?;

    Ok(scale_by_decimals(amount, 9))
}

/// A minijinja filter that formats a given amount in its atomic unit into USDC.
pub fn usdc(_state: &State, value: String) -> Result<String, Error> {
    let amount = Decimal::from_str(&value).map_err(|e| {
        Error::new(ErrorKind::InvalidOperation, format!("Failed to parse amount: {e}"))
    })?;

    Ok(scale_by_decimals(amount, 6))
}

/// A minijinja filter that formats a given amount in its atomic unit into USDT.
pub fn usdt(_state: &State, value: String) -> Result<String, Error> {
    let amount = Decimal::from_str(&value).map_err(|e| {
        Error::new(ErrorKind::InvalidOperation, format!("Failed to parse amount: {e}"))
    })?;

    Ok(scale_by_decimals(amount, 6))
}

/// A minijinja filter that formats a given amount in its atomic unit into WBTC.
pub fn wbtc(_state: &State, value: String) -> Result<String, Error> {
    let amount = Decimal::from_str(&value).map_err(|e| {
        Error::new(ErrorKind::InvalidOperation, format!("Failed to parse amount: {e}"))
    })?;

    Ok(scale_by_decimals(amount, 8))
}

/// Scales a Decimal value by a given number of decimals and returns it as a
/// formatted string.
///
/// This function is primarily used for converting large integer representations
/// of token amounts (e.g., in wei) into a decimal string format (e.g., in
/// ether).
///
/// # Arguments
///
/// * `amount` - The Decimal value to be scaled.
/// * `decimals` - The number of decimal places to scale by (e.g., 18 for ETH).
///
/// # Returns
///
/// A string representing the scaled decimal value. Trailing zeros are trimmed.
fn scale_by_decimals(amount: Decimal, decimals: u32) -> String {
    let scaling_factor = Decimal::from(10).powi(decimals.into());
    let scaled_amount = amount / scaling_factor;

    scaled_amount.normalize().to_string()
}

#[cfg(test)]
mod tests {
    use minijinja::Environment;

    use super::*;

    #[test]
    fn test_sum_filter() {
        let values = Value::from_serialize(&vec!["1000", "2000", "3000.5"]);
        let result = sum(values).unwrap();
        assert_eq!(result, "6000.5");
    }

    #[test]
    fn test_avg_filter() {
        let values = Value::from_serialize(&vec!["10", "20", "30"]);
        let result = avg(values).unwrap();
        assert_eq!(result, "20");
    }

    #[test]
    fn test_decimals() {
        let env = Environment::new();
        let state = env.empty_state();
        let value = "1234500000000000000".to_string();
        let result = decimals(&state, value, 18).unwrap();
        assert_eq!(result, "1.2345");
    }

    #[test]
    fn test_gwei() {
        let env = Environment::new();
        let state = env.empty_state();
        let value = "1000000000".to_string();
        let result = gwei(&state, value).unwrap();
        assert_eq!(result, "1");
    }

    #[test]
    fn test_ether() {
        let env = Environment::new();
        let state = env.empty_state();
        let value = "1000000000000000000".to_string();
        let result = ether(&state, value).unwrap();
        assert_eq!(result, "1");
    }

    #[test]
    fn test_usdc() {
        let env = Environment::new();
        let state = env.empty_state();
        let value = "1000000".to_string();
        let result = usdc(&state, value).unwrap();
        assert_eq!(result, "1");
    }

    #[test]
    fn test_usdt() {
        let env = Environment::new();
        let state = env.empty_state();
        let value = "1000000".to_string();
        let result = usdt(&state, value).unwrap();
        assert_eq!(result, "1");
    }

    #[test]
    fn test_wbtc() {
        let env = Environment::new();
        let state = env.empty_state();
        let value = "100000000".to_string();
        let result = wbtc(&state, value).unwrap();
        assert_eq!(result, "1");
    }
}
