//! Handlers for action-related endpoints in the HTTP server.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde_json::json;

use super::{ApiError, ApiState};

/// Retrieves all actions from the database and returns them as a JSON response.
pub async fn actions(State(state): State<ApiState>) -> Result<impl IntoResponse, ApiError> {
    let actions = state.repo.get_actions(&state.config.network_id).await?;
    Ok((StatusCode::OK, Json(json!({ "actions": actions }))))
}

/// Retrieves details of a specific action by its ID.
pub async fn action_details(
    State(state): State<ApiState>,
    Path(action_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let action = state
        .repo
        .get_action_by_id(&state.config.network_id, &action_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Action not found".to_string()))?;

    Ok((StatusCode::OK, Json(json!({ "action": action }))))
}
