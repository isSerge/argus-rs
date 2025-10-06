//! HTTP server module

use std::{net::SocketAddr, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::get,
};
use reqwest::StatusCode;
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
async fn monitors(State(state): State<ApiState>) -> impl IntoResponse {
    match state.repo.get_monitors(&state.config.network_id).await {
        Ok(monitors) => (StatusCode::OK, Json(json!({ "monitors": monitors }))).into_response(),
        Err(e) => {
            tracing::error!("Failed to retrieve monitors: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to retrieve monitors" })),
            )
                .into_response()
        }
    }
}

/// Retrieves details of a specific monitor by its ID.
async fn monitor_details(
    State(state): State<ApiState>,
    Path(monitor_id): Path<String>,
) -> impl IntoResponse {
    match state.repo.get_monitor_by_id(&state.config.network_id, &monitor_id).await {
        Ok(Some(monitor)) => (StatusCode::OK, Json(json!({ "monitor": monitor }))).into_response(),
        Ok(None) =>
            (StatusCode::NOT_FOUND, Json(json!({ "error": "Monitor not found" }))).into_response(),
        Err(e) => {
            tracing::error!("Failed to retrieve monitor {}: {}", monitor_id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to retrieve monitor" })),
            )
                .into_response()
        }
    }
}

/// Runs the HTTP server based on the provided application configuration.
pub async fn run_server_from_config(config: Arc<AppConfig>, repo: Arc<dyn AppRepository>) {
    let addr: SocketAddr =
        config.server.listen_address.parse().expect("Invalid api_server.listen_address format");

    let state = ApiState { config, repo };

    let app = Router::new()
        .route("/health", get(health))
        .route("/monitors", get(monitors))
        .route("/monitors/{id}", get(monitor_details))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await.expect("Failed to bind address");

    tracing::info!("HTTP server listening on {}", addr);

    axum::serve(listener, app.into_make_service()).await.expect("Server failed");
}
