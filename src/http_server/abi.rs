//! ABI management HTTP handlers

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde_json::json;

use super::{ApiError, ApiState};

/// Request payload for uploading an ABI.
#[derive(serde::Deserialize)]
pub struct UploadAbiRequest {
    /// The name of the ABI.
    pub name: String,
    /// The ABI content in JSON format.
    pub abi: String,
}

/// Handles the upload of a new ABI.
pub async fn upload_abi(
    State(state): State<ApiState>,
    Json(payload): Json<UploadAbiRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let abi_json = serde_json::from_str::<serde_json::Value>(&payload.abi)
        .map_err(|e| ApiError::UnprocessableEntity(format!("Invalid ABI JSON: {}", e)))?;

    // Check if ABI already exists
    if state.repo.get_abi(&payload.name).await?.is_some() {
        return Err(ApiError::Conflict(format!(
            "ABI with name '{}' already exists.",
            payload.name
        )));
    }

    state.repo.create_abi(&payload.name, &payload.abi).await?;

    Ok((
        StatusCode::CREATED,
        Json(json!({ "abi": { "name": payload.name, "abi": payload.abi } })),
    ))
}

/// Retrieves an ABI by its name.
pub async fn get_abi_by_name(
    State(state): State<ApiState>,
    Path(abi_name): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let abi_content = state
        .repo
        .get_abi(&abi_name)
        .await?
        .ok_or_else(|| ApiError::NotFound("ABI not found".to_string()))?;

    Ok((StatusCode::OK, Json(json!({ "abi": { "name": abi_name, "abi": abi_content } }))))
}

/// Lists all available ABIs.
pub async fn list_abis(State(state): State<ApiState>) -> Result<impl IntoResponse, ApiError> {
    let abis = state.repo.list_abis().await?;

    Ok((StatusCode::OK, Json(json!({ "abis": abis }))))
}

/// Deletes an ABI by its name.
pub async fn delete_abi(
    State(state): State<ApiState>,
    Path(abi_name): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    // Check if ABI exists
    if state.repo.get_abi(&abi_name).await?.is_none() {
        return Err(ApiError::NotFound("ABI not found".to_string()));
    }

    // Check if any monitors are using this ABI
    let network_id = &state.config.network_id;
    let all_monitors = state.repo.get_monitors(network_id).await?;
    let monitors_using_abi: Vec<String> = all_monitors
        .into_iter()
        .filter(|m| m.abi_name.as_deref() == Some(&abi_name))
        .map(|m| m.name)
        .collect();

    if !monitors_using_abi.is_empty() {
        return Err(ApiError::AbiInUse(monitors_using_abi));
    }

    state.repo.delete_abi(&abi_name).await?;

    Ok((StatusCode::NO_CONTENT, Json(json!({}))))
}
