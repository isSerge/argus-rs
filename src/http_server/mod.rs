//! HTTP server module

use axum::{routing::get, Router, response::IntoResponse, Json};
use serde_json::json;
use std::net::SocketAddr;

use crate::config::AppConfig;

async fn health() -> impl IntoResponse {
	Json(json!({ "status": "ok" }))
}

pub async fn run_server_from_config(config: &AppConfig) {
	let addr: SocketAddr = config.api_server_listen_address.parse()
		.expect("Invalid api_server.listen_address format");
	let app = Router::new()
		.route("/health", get(health));

	axum::Server::bind(&addr)
		.serve(app.into_make_service())
		.await
		.expect("server failed");
}
