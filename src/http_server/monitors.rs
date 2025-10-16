//! Handlers for monitor-related endpoints in the HTTP server.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde_json::json;

use super::{ApiError, ApiState};

/// Retrieves all monitors from the database and returns them as a JSON
/// response.
pub async fn monitors(State(state): State<ApiState>) -> Result<impl IntoResponse, ApiError> {
    let monitors = state.repo.get_monitors(&state.config.network_id).await?;
    Ok((StatusCode::OK, Json(json!({ "monitors": monitors }))))
}

/// Retrieves details of a specific monitor by its ID.
pub async fn monitor_details(
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

/// For now just triggers an update of monitors by notifying configuration
/// changes. In the future, should accept a payload to add monitors.
pub async fn update_monitors(State(state): State<ApiState>) -> Result<impl IntoResponse, ApiError> {
    tracing::debug!("Received request to update monitors");
    // Notify configuration changes
    if state.config_tx.send(()).is_err() {
        tracing::error!("Failed to notify configuration change");
        return Err(ApiError::InternalServerError(
            "Failed to notify configuration change".to_string(),
        ));
    }

    Ok((StatusCode::OK, Json(json!({ "status": "Monitors update triggered" }))))
}
