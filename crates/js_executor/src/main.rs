mod engine;

use std::{
    env,
    io::{Write, stdout},
    path::PathBuf,
};

use axum::{Json, Router, routing::post};
use clap::Parser;
use common_models::{ErrorResponse, ExecutionRequest, ExecutionResponse};
use tokio::net::UnixListener;
use tower_http::trace::TraceLayer;

use crate::engine::{JsRunnerError, execute_script};

#[derive(Parser, Debug)]
#[command(name = "js_executor")]
#[command(about = "JavaScript executor service for Argus")]
struct Args {
    /// Path for the Unix domain socket
    #[arg(long)]
    socket_path: Option<PathBuf>,
}

impl From<JsRunnerError> for ErrorResponse {
    fn from(err: JsRunnerError) -> Self {
        ErrorResponse { error: err.to_string() }
    }
}

/// Generates a unique path for the Unix Domain Socket.
fn get_socket_path() -> PathBuf {
    let pid = std::process::id();
    env::temp_dir().join(format!("argus-js-executor-{}.sock", pid))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let app =
        Router::new().route("/execute", post(execute_handler)).layer(TraceLayer::new_for_http());

    let socket_path = args.socket_path.unwrap_or_else(get_socket_path);

    // Clean up any existing socket file before binding
    if socket_path.exists() {
        std::fs::remove_file(&socket_path).expect("Failed to remove existing socket file");
    }

    let listener = UnixListener::bind(&socket_path).expect("Failed to bind to Unix socket");

    // Print the socket path to stdout for the client to read and flush immediately
    println!("Listening on {}", socket_path.display());
    stdout().flush().unwrap();

    tracing::info!("Listening on {}", socket_path.display());

    axum::serve(listener, app.into_make_service()).await.unwrap();
}

async fn execute_handler(
    Json(payload): Json<ExecutionRequest>,
) -> Result<Json<ExecutionResponse>, Json<ErrorResponse>> {
    match execute_script(payload.script, &payload.context).await {
        Ok(result) => Ok(Json(result)),
        Err(e) => Err(Json(e.into())),
    }
}
