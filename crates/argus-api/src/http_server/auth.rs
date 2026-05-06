//! Authentication middleware for the HTTP server.

use axum::{
    body::Body,
    extract::State,
    http::{Request, header},
    middleware::Next,
    response::Response,
};

use super::{ApiState, error::ApiError};

/// Middleware for authenticating requests using a bearer token.
pub async fn auth(
    State(state): State<ApiState>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, ApiError> {
    let auth_header =
        request.headers().get(header::AUTHORIZATION).and_then(|header| header.to_str().ok());

    let token = auth_header
        .and_then(|header| header.strip_prefix("Bearer "))
        .ok_or(ApiError::Unauthorized)?;

    let api_key = state.config.server.api_key.as_ref().expect("API key must be configured");

    if token == api_key { Ok(next.run(request).await) } else { Err(ApiError::Unauthorized) }
}
