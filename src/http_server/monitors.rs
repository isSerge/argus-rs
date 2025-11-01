//! Handlers for monitor-related endpoints in the HTTP server.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde_json::json;

use super::{ApiError, ApiState};
use crate::models::monitor::MonitorConfig;

/// Retrieves all monitors from the database and returns them as a JSON
/// response.
pub async fn get_monitors(State(state): State<ApiState>) -> Result<impl IntoResponse, ApiError> {
    let monitors = state.repo.get_monitors(&state.config.network_id).await?;
    Ok((StatusCode::OK, Json(json!({ "monitors": monitors }))))
}

/// Retrieves details of a specific monitor by its ID.
pub async fn get_monitor_details(
    State(state): State<ApiState>,
    Path(monitor_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let monitor = state
        .repo
        .get_monitor_by_id(&state.config.network_id, &monitor_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Monitor not found".to_string()))?;

    Ok((StatusCode::OK, Json(json!({ "monitor": monitor }))))
}

/// Updates monitor based on the provided payload.
pub async fn update_monitor(
    State(state): State<ApiState>,
    Path(monitor_id): Path<String>,
    Json(payload): Json<MonitorConfig>,
) -> Result<impl IntoResponse, ApiError> {
    // Validate payload (persistence + business logic) using shared persistence
    // validator
    state.monitor_persistence_validator.validate_for_update(&monitor_id, &payload).await?;

    // save updated monitor to the database
    state.repo.update_monitor(&state.config.network_id, &monitor_id, payload).await?;

    // Notify configuration changes
    if state.config_tx.send(()).is_err() {
        tracing::error!("Failed to notify configuration change");
        return Err(ApiError::InternalServerError(
            "Failed to notify configuration change".to_string(),
        ));
    }

    Ok((StatusCode::OK, Json(json!({ "status": "Monitors update triggered" }))))
}

/// Creates a new monitor based on the provided payload.
pub async fn create_monitor(
    State(state): State<ApiState>,
    Json(payload): Json<MonitorConfig>,
) -> Result<impl IntoResponse, ApiError> {
    // Validate payload (persistence + business logic) using shared persistence
    // validator
    state.monitor_persistence_validator.validate_for_create(&payload).await?;

    // save new monitor to the database
    state.repo.add_monitors(&state.config.network_id, vec![payload]).await?;

    // Notify configuration changes
    if state.config_tx.send(()).is_err() {
        tracing::error!("Failed to notify configuration change");
        return Err(ApiError::InternalServerError(
            "Failed to notify configuration change".to_string(),
        ));
    }

    Ok((StatusCode::CREATED, Json(json!({ "status": "Monitor creation triggered" }))))
}

/// Deletes a monitor by its ID.
pub async fn delete_monitor(
    State(state): State<ApiState>,
    Path(monitor_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    // delete monitor from the database
    state.repo.delete_monitor(&state.config.network_id, &monitor_id).await?;

    // Notify configuration changes
    if state.config_tx.send(()).is_err() {
        tracing::error!("Failed to notify configuration change");
        return Err(ApiError::InternalServerError(
            "Failed to notify configuration change".to_string(),
        ));
    }

    Ok((StatusCode::NO_CONTENT, Json(json!({ "status": "Monitor deletion triggered" }))))
}
