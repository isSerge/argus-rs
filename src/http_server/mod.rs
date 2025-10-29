//! HTTP server module

mod abi;
mod actions;
mod auth;
mod error;
mod monitors;
mod status;

use std::{net::SocketAddr, sync::Arc};

use abi::{delete_abi, get_abi_by_name, get_abis, upload_abi};
use actions::{create_action, delete_action, get_action_details, get_actions, update_action};
use auth::auth;
use axum::{
    Router, middleware,
    response::{IntoResponse, Json},
    routing::{delete, get, post, put},
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

    if config.server.api_key.is_none() {
        panic!("`server.api_key` or `ARGUS_API_KEY` must be set to run the API server");
    }

    let state = ApiState { config, repo, app_metrics, config_tx };

    let app = Router::new()
        .route("/health", get(health))
        .route("/status", get(status))
        // Monitors routes
        .route(
            "/monitors",
            post(update_monitors).route_layer(middleware::from_fn_with_state(state.clone(), auth)),
        )
        .route("/monitors", get(monitors))
        .route("/monitors/{id}", get(monitor_details))
        // ABI routes
        .route(
            "/abis",
            post(upload_abi).route_layer(middleware::from_fn_with_state(state.clone(), auth)),
        )
        .route("/abis", get(get_abis))
        .route("/abis/{name}", get(get_abi_by_name))
        .route(
            "/abis/{name}",
            delete(delete_abi).route_layer(middleware::from_fn_with_state(state.clone(), auth)),
        )
        // Actions routes
        .route("/actions", get(get_actions))
        .route("/actions/{id}", get(get_action_details))
        .route(
            "/actions",
            post(create_action).route_layer(middleware::from_fn_with_state(state.clone(), auth)),
        )
        .route(
            "/actions/{id}",
            put(update_action).route_layer(middleware::from_fn_with_state(state.clone(), auth)),
        )
        .route(
            "/actions/{id}",
            delete(delete_action).route_layer(middleware::from_fn_with_state(state.clone(), auth)),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await.expect("Failed to bind address");

    tracing::info!("HTTP server listening on {}", addr);

    axum::serve(listener, app.into_make_service()).await.expect("Server failed");
}
