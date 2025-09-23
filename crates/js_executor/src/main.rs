mod engine;

use argus_models::monitor_match::MonitorMatch;
use axum::{Json, Router, http::StatusCode, routing::post};
use serde::{Deserialize, Serialize};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let app = Router::new().route("/execute", post(execute_action_handler));
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();

    axum::serve(listener, app).await.unwrap();
}

#[derive(Deserialize)]
struct ExecuteActionRequest {
    script: String,
    context: MonitorMatch,
}

#[derive(Serialize)]
struct ExecuteActionResponse {
    modified_match: MonitorMatch,
}

async fn execute_action_handler(
    Json(payload): Json<ExecuteActionRequest>,
) -> (StatusCode, Json<ExecuteActionResponse>) {
    let context = payload.context;
    let script = payload.script;

    match engine::execute_script(script, &context).await {
        Ok(modified_match) => (StatusCode::OK, Json(ExecuteActionResponse { modified_match })),
        Err(err) => {
            tracing::error!("Error executing action: {}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ExecuteActionResponse { modified_match: context }),
            )
        }
    }
}
