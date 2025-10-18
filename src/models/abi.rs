//! Data models for ABI management.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Represents an ABI stored in the database.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Abi {
    /// Unique name/identifier for the ABI (e.g., "erc20", "aave_governance_v2")
    pub name: String,

    /// The ABI JSON content as a string
    pub abi_content: String,

    /// Timestamp when the ABI was created
    pub created_at: DateTime<Utc>,

    /// Timestamp when the ABI was last updated
    pub updated_at: DateTime<Utc>,
}

/// Request payload for uploading a new ABI
#[derive(Debug, Clone, Deserialize)]
pub struct CreateAbiRequest {
    /// Unique name for the ABI
    pub name: String,

    /// The ABI JSON content (should be valid JSON array)
    pub abi: serde_json::Value,
}

/// Response after creating an ABI
#[derive(Debug, Clone, Serialize)]
pub struct CreateAbiResponse {
    /// The name of the created ABI
    pub name: String,
}

/// Response containing ABI details
#[derive(Debug, Clone, Serialize)]
pub struct AbiResponse {
    /// The name of the ABI
    pub name: String,

    /// The ABI JSON content
    pub abi: serde_json::Value,

    /// Timestamp when the ABI was created
    pub created_at: DateTime<Utc>,

    /// Timestamp when the ABI was last updated
    pub updated_at: DateTime<Utc>,
}

impl From<Abi> for AbiResponse {
    fn from(abi: Abi) -> Self {
        let abi_json = serde_json::from_str(&abi.abi_content)
            .unwrap_or(serde_json::Value::Array(vec![]));
        
        Self {
            name: abi.name,
            abi: abi_json,
            created_at: abi.created_at,
            updated_at: abi.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_abi_request_deserialization() {
        let json = r#"{
            "name": "erc20",
            "abi": [{"type": "function", "name": "transfer"}]
        }"#;

        let request: CreateAbiRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.name, "erc20");
        assert!(request.abi.is_array());
    }

    #[test]
    fn test_abi_response_from_abi() {
        let abi = Abi {
            name: "test".to_string(),
            abi_content: r#"[{"type": "function", "name": "test"}]"#.to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let response: AbiResponse = abi.into();
        assert_eq!(response.name, "test");
        assert!(response.abi.is_array());
    }
}
