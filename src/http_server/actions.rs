//! Handlers for action-related endpoints in the HTTP server.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde_json::json;

use super::{ApiError, ApiState};
use crate::{action::validator::ActionValidator, models::action::ActionConfig};

/// Retrieves all actions from the database and returns them as a JSON response.
pub async fn get_actions(State(state): State<ApiState>) -> Result<impl IntoResponse, ApiError> {
    let actions = state.repo.get_actions(&state.config.network_id).await?;
    Ok((StatusCode::OK, Json(json!({ "actions": actions }))))
}

/// Retrieves details of a specific action by its ID.
pub async fn get_action_details(
    State(state): State<ApiState>,
    Path(action_id): Path<i64>,
) -> Result<impl IntoResponse, ApiError> {
    let action = state
        .repo
        .get_action_by_id(&state.config.network_id, action_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Action not found".to_string()))?;

    Ok((StatusCode::OK, Json(json!({ "action": action }))))
}

/// Creates a new action based on the provided details.
pub async fn create_action(
    State(state): State<ApiState>,
    Json(action): Json<ActionConfig>,
) -> Result<impl IntoResponse, ApiError> {
    let validator = ActionValidator::new(state.repo.clone(), &state.config.network_id);
    validator.validate_for_create(&action).await?;

    let new_action = state.repo.create_action(&state.config.network_id, action).await?;

    // Notify that the configuration has changed
    state.config_tx.send(()).ok();

    Ok((StatusCode::CREATED, Json(json!({ "action": new_action }))))
}

/// Updates an existing action.
pub async fn update_action(
    State(state): State<ApiState>,
    Path(action_id): Path<i64>,
    Json(mut action): Json<ActionConfig>,
) -> Result<impl IntoResponse, ApiError> {
    // Ensure the action ID from the path matches the one in the body
    action.id = Some(action_id);

    let validator = ActionValidator::new(state.repo.clone(), &state.config.network_id);
    validator.validate_for_update(&action).await?;

    let updated_action = state.repo.update_action(&state.config.network_id, action).await?;

    // Notify that the configuration has changed
    state.config_tx.send(()).ok();

    Ok((StatusCode::OK, Json(json!({ "action": updated_action }))))
}

/// Deletes an action by its ID.
pub async fn delete_action(
    State(state): State<ApiState>,
    Path(action_id): Path<i64>,
) -> Result<impl IntoResponse, ApiError> {
    state.repo.delete_action(&state.config.network_id, action_id).await?;

    // Notify that the configuration has changed
    state.config_tx.send(()).ok();

    Ok(StatusCode::NO_CONTENT)
}
