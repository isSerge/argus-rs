use alloy::dyn_abi::DynSolValue;
use serde::Serialize;

use super::Log;

/// A decoded Ethereum contract event (log).
///
/// This struct combines the raw log data with a human-readable name and the
/// decoded parameter values from the contract's ABI.
#[derive(Debug, Clone)]
pub struct DecodedLog {
    /// The original raw log.
    pub log: Log,
    /// The name of the event, as defined in the ABI (e.g., "Transfer").
    pub name: String,
    /// The decoded parameter values from the event.
    pub params: Vec<(String, DynSolValue)>,
}

impl Serialize for DecodedLog {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        const FIELD_COUNT: usize = 3;
        let mut state = serializer.serialize_struct("DecodedLog", FIELD_COUNT)?;
        state.serialize_field("log", &self.log)?;
        state.serialize_field("name", &self.name)?;

        // Manually serialize Vec<(String, DynSolValue)> as a JSON object
        let mut params_map = std::collections::HashMap::new();
        for (name, value) in &self.params {
            params_map.insert(name, dyn_sol_value_to_json(value));
        }
        state.serialize_field("params", &params_map)?;
        state.end()
    }
}

/// A decoded Ethereum contract function call.
///
/// This struct holds the function name and its decoded input parameters
/// from the contract's ABI.
#[derive(Debug, Clone)]
pub struct DecodedCall {
    /// The name of the function, as defined in the ABI (e.g., "transfer").
    pub name: String,
    /// The decoded input parameters for the function.
    pub params: Vec<(String, DynSolValue)>,
}

impl Serialize for DecodedCall {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        const FIELD_COUNT: usize = 2;
        let mut state = serializer.serialize_struct("DecodedCall", FIELD_COUNT)?;
        state.serialize_field("name", &self.name)?;

        // Manually serialize Vec<(String, DynSolValue)> as a JSON object
        let mut params_map = std::collections::HashMap::new();
        for (name, value) in &self.params {
            params_map.insert(name, dyn_sol_value_to_json(value));
        }
        state.serialize_field("params", &params_map)?;
        state.end()
    }
}

/// A helper function to convert `DynSolValue` to a JSON-compatible
/// representation. This is used for serializing `DecodedCall` and `DecodedLog`
/// parameters.
pub fn dyn_sol_value_to_json(value: &DynSolValue) -> serde_json::Value {
    match value {
        DynSolValue::Address(a) => serde_json::Value::String(a.to_checksum(None)),
        DynSolValue::Bool(b) => serde_json::Value::Bool(*b),
        DynSolValue::Bytes(b) => serde_json::Value::String(format!("0x{}", hex::encode(b))),
        DynSolValue::FixedBytes(fb, _) =>
            serde_json::Value::String(format!("0x{}", hex::encode(fb))),
        DynSolValue::Int(i, _) => serde_json::Value::String(i.to_string()),
        DynSolValue::Uint(u, _) => serde_json::Value::String(u.to_string()),
        DynSolValue::String(s) => serde_json::Value::String(s.clone()),
        DynSolValue::Array(a) =>
            serde_json::Value::Array(a.iter().map(dyn_sol_value_to_json).collect()),
        DynSolValue::FixedArray(fa) =>
            serde_json::Value::Array(fa.iter().map(dyn_sol_value_to_json).collect()),
        DynSolValue::Tuple(t) =>
            serde_json::Value::Array(t.iter().map(dyn_sol_value_to_json).collect()),
        _ => serde_json::Value::Null,
    }
}
