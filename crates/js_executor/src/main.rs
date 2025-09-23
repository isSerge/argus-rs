mod engine;

use argus_models::monitor_match::MonitorMatch;
use axum::{Json, Router, routing::post};
use serde::{Deserialize, Serialize};
use tower_http::trace::TraceLayer;

use crate::engine::{JsRunnerError, execute_script};

// TODO: move to argus_models
#[derive(Deserialize)]
struct ExecutionRequest {
    script: String,
    context: MonitorMatch,
}

// TODO: move to argus_models
#[derive(Serialize)]
struct ExecutionResponse {
    result: MonitorMatch,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

impl From<JsRunnerError> for ErrorResponse {
    fn from(err: JsRunnerError) -> Self {
        ErrorResponse { error: err.to_string() }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let app =
        Router::new().route("/execute", post(execute_handler)).layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tracing::info!("Listening on {}", addr.port());

    axum::serve(listener, app).await.unwrap();
}

async fn execute_handler(
    Json(payload): Json<ExecutionRequest>,
) -> Result<Json<ExecutionResponse>, Json<ErrorResponse>> {
    match execute_script(payload.script, &payload.context).await {
        Ok(result) => Ok(Json(ExecutionResponse { result })),
        Err(e) => Err(Json(e.into())),
    }
}
