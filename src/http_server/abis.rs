//! Handlers for ABI-related endpoints in the HTTP server.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde_json::json;

use super::{ApiError, ApiState};
use crate::models::abi::{AbiResponse, CreateAbiRequest, CreateAbiResponse};

/// Retrieves all ABIs from the database and returns them as a JSON response.
pub async fn get_abis(State(state): State<ApiState>) -> Result<impl IntoResponse, ApiError> {
    let abis = state.repo.get_abis().await?;
    
    let abi_responses: Vec<AbiResponse> = abis.into_iter().map(|abi| abi.into()).collect();
    
    Ok((StatusCode::OK, Json(json!({ "abis": abi_responses }))))
}

/// Retrieves details of a specific ABI by its name.
pub async fn get_abi_by_name(
    State(state): State<ApiState>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let abi = state
        .repo
        .get_abi_by_name(&name)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("ABI '{}' not found", name)))?;

    let abi_response: AbiResponse = abi.into();
    Ok((StatusCode::OK, Json(abi_response)))
}

/// Creates a new ABI in the database.
pub async fn create_abi(
    State(state): State<ApiState>,
    Json(payload): Json<CreateAbiRequest>,
) -> Result<impl IntoResponse, ApiError> {
    // Validate ABI name (alphanumeric, underscores, hyphens only)
    if !payload.name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
        return Err(ApiError::BadRequest(
            "ABI name can only contain alphanumeric characters, underscores, and hyphens"
                .to_string(),
        ));
    }

    // Validate that the ABI is a valid JSON array
    if !payload.abi.is_array() {
        return Err(ApiError::BadRequest("ABI must be a JSON array".to_string()));
    }

    // Serialize the ABI to a string
    let abi_content =
        serde_json::to_string(&payload.abi).map_err(|e| ApiError::BadRequest(e.to_string()))?;

    // Add the ABI to the database
    state.repo.add_abi(&payload.name, &abi_content).await?;

    let response = CreateAbiResponse { name: payload.name };
    Ok((StatusCode::CREATED, Json(response)))
}

/// Deletes an ABI by its name.
pub async fn delete_abi(
    State(state): State<ApiState>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    state.repo.delete_abi(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Lists all ABI names.
pub async fn list_abi_names(State(state): State<ApiState>) -> Result<impl IntoResponse, ApiError> {
    let abis = state.repo.get_abis().await?;
    let names: Vec<String> = abis.into_iter().map(|abi| abi.name).collect();
    
    Ok((StatusCode::OK, Json(json!({ "names": names }))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_abi_name() {
        let valid_names = vec!["erc20", "my_abi", "test-abi", "ABI123"];
        for name in valid_names {
            assert!(name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-'));
        }

        let invalid_names = vec!["my abi", "abi@123", "test.abi"];
        for name in invalid_names {
            assert!(!name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-'));
        }
    }
}
