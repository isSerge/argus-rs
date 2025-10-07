//! HTTP server module

mod error;

use std::{net::SocketAddr, sync::Arc};

use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
};
use error::ApiError;
use serde_json::json;

use crate::{config::AppConfig, persistence::traits::AppRepository};

/// Shared application state for the HTTP server.
#[derive(Clone)]
pub struct ApiState {
    /// The application configuration.
    config: Arc<AppConfig>,
    /// The application repository.
    repo: Arc<dyn AppRepository>,
}

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

/// Retrieves all monitors from the database and returns them as a JSON
/// response.
async fn monitors(State(state): State<ApiState>) -> Result<impl IntoResponse, ApiError> {
    let monitors = state.repo.get_monitors(&state.config.network_id).await?;
    Ok((StatusCode::OK, Json(json!({ "monitors": monitors }))))
}

/// Retrieves details of a specific monitor by its ID.
async fn monitor_details(
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

/// Retrieves all actions from the database and returns them as a JSON response.
async fn actions(State(state): State<ApiState>) -> Result<impl IntoResponse, ApiError> {
    let actions = state.repo.get_actions(&state.config.network_id).await?;
    Ok((StatusCode::OK, Json(json!({ "actions": actions }))))
}

/// Retrieves details of a specific action by its ID.
async fn action_details(
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

/// Runs the HTTP server based on the provided application configuration.
pub async fn run_server_from_config(config: Arc<AppConfig>, repo: Arc<dyn AppRepository>) {
    let addr: SocketAddr =
        config.server.listen_address.parse().expect("Invalid server.listen_address format");

    let state = ApiState { config, repo };

    let app = Router::new()
        .route("/health", get(health))
        .route("/monitors", get(monitors))
        .route("/monitors/{id}", get(monitor_details))
        .route("/actions", get(actions))
        .route("/actions/{id}", get(action_details))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await.expect("Failed to bind address");

    tracing::info!("HTTP server listening on {}", addr);

    axum::serve(listener, app.into_make_service()).await.expect("Server failed");
}
