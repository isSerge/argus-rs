//! HTTP server module

mod abis;
mod actions;
mod auth;
mod error;
mod monitors;
mod status;

use std::{net::SocketAddr, sync::Arc};

use abis::{create_abi, delete_abi, get_abi_by_name, get_abis, list_abi_names};
use actions::{action_details, actions};
use auth::auth;
use axum::{
    Router, middleware,
    response::{IntoResponse, Json},
    routing::{delete, get, post},
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
        .route(
            "/monitors",
            post(update_monitors).route_layer(middleware::from_fn_with_state(state.clone(), auth)),
        )
        .route("/monitors", get(monitors))
        .route("/monitors/{id}", get(monitor_details))
        .route("/actions", get(actions))
        .route("/actions/{id}", get(action_details))
        // ABI endpoints
        .route(
            "/abis",
            post(create_abi).route_layer(middleware::from_fn_with_state(state.clone(), auth)),
        )
        .route("/abis", get(list_abi_names))
        .route("/abis/all", get(get_abis))
        .route("/abis/{name}", get(get_abi_by_name))
        .route(
            "/abis/{name}",
            delete(delete_abi).route_layer(middleware::from_fn_with_state(state.clone(), auth)),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await.expect("Failed to bind address");

    tracing::info!("HTTP server listening on {}", addr);

    axum::serve(listener, app.into_make_service()).await.expect("Server failed");
}
