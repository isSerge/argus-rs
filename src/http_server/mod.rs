//! HTTP server module

use std::net::SocketAddr;

use axum::{Json, Router, response::IntoResponse, routing::get};
use serde_json::json;

use crate::config::AppConfig;

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

/// Runs the HTTP server based on the provided application configuration.
pub async fn run_server_from_config(config: &AppConfig) {
    let addr: SocketAddr =
        config.api_server_listen_address.parse().expect("Invalid api_server.listen_address format");
    
    let app = Router::new().route("/health", get(health));
    
    let listener = tokio::net::TcpListener::bind(addr).await.expect("Failed to bind address");
    
    axum::serve(listener, app.into_make_service()).await.expect("Server failed");
}
