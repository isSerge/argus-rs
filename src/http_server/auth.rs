//! Authentication middleware for the HTTP server.

use axum::{
    body::Body,
    extract::State,
    http::{header, Request},
    middleware::Next,
    response::Response,
};

use super::{error::ApiError, ApiState};

/// Middleware for authenticating requests using a bearer token.
pub async fn auth(
    State(state): State<ApiState>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, ApiError> {
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|header| header.to_str().ok());

    let token = auth_header
        .and_then(|header| header.strip_prefix("Bearer "))
        .ok_or(ApiError::Unauthorized)?;

    if let Some(api_key) = &state.config.server.api_key {
        if token == api_key {
            Ok(next.run(request).await)
        } else {
            Err(ApiError::Unauthorized)
        }
    } else {
        Err(ApiError::InternalServerError("API key not configured".to_string()))
    }
}
