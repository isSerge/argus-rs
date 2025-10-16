//! HTTP server module

mod actions;
mod error;
mod monitors;
mod status;

use std::{net::SocketAddr, sync::Arc};

use actions::{action_details, actions};
use axum::{
    Router,
    response::{IntoResponse, Json},
    routing::{get, post},
};
use error::ApiError;
use monitors::{monitor_details, monitors, update_monitors};
use serde_json::json;
use status::status;
use tokio::sync::watch;

use crate::{config::AppConfig, context::AppMetrics, persistence::traits::AppRepository};

/// Shared application state for the HTTP server.
#[derive(Clone)]
pub struct ApiState {
    /// The application configuration.
    config: Arc<AppConfig>,
    /// The application repository.
    repo: Arc<dyn AppRepository>,
    /// The application metrics.
    app_metrics: AppMetrics,
    /// A channel to notify configuration changes.
    config_tx: watch::Sender<()>,
}

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

/// Runs the HTTP server based on the provided application configuration.
pub async fn run_server_from_config(
    config: Arc<AppConfig>,
    repo: Arc<dyn AppRepository>,
    app_metrics: AppMetrics,
    config_tx: watch::Sender<()>,
) {
    let addr: SocketAddr =
        config.server.listen_address.parse().expect("Invalid server.listen_address format");

    let state = ApiState { config, repo, app_metrics, config_tx };

    let app = Router::new()
        .route("/health", get(health))
        .route("/status", get(status))
        .route("/monitors", post(update_monitors))
        .route("/monitors", get(monitors))
        .route("/monitors/{id}", get(monitor_details))
        .route("/actions", get(actions))
        .route("/actions/{id}", get(action_details))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await.expect("Failed to bind address");

    tracing::info!("HTTP server listening on {}", addr);

    axum::serve(listener, app.into_make_service()).await.expect("Server failed");
}
