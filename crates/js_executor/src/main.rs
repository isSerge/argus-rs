mod engine;

use axum::{Json, Router, routing::post};
use common_models::{ErrorResponse, ExecutionRequest, ExecutionResponse};
use tower_http::trace::TraceLayer;

use crate::engine::{JsRunnerError, execute_script};

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
    // Print the port to stdout for the client to read
    println!("{}", addr.port());
    // Log the listening address to stderr
    tracing::info!("Listening on {}", addr);

    axum::serve(listener, app).await.unwrap();
}

async fn execute_handler(
    Json(payload): Json<ExecutionRequest>,
) -> Result<Json<ExecutionResponse>, Json<ErrorResponse>> {
    match execute_script(payload.script, &payload.context).await {
        Ok(result) => Ok(Json(result)),
        Err(e) => Err(Json(e.into())),
    }
}
