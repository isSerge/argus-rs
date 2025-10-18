//! Defines the custom `ApiError` type for the HTTP server.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde_json::json;

use crate::persistence::error::PersistenceError;

/// A custom error type for the API that can be converted into an HTTP response.
pub enum ApiError {
    /// Represents an unauthorized request.
    Unauthorized,

    /// Represents a resource that could not be found.
    NotFound(String),

    /// Represents a conflict (e.g., duplicate resource).
    Conflict(String),

    /// Represents a bad request (e.g., invalid input).
    BadRequest(String),

    /// Represents a generic internal server error.
    InternalServerError(String),
}

/// Converts a `PersistenceError` into an `ApiError`.
///
/// This allows for the convenient use of the `?` operator in handlers
/// on functions that return `Result<_, PersistenceError>`.
impl From<PersistenceError> for ApiError {
    fn from(err: PersistenceError) -> Self {
        match err {
            PersistenceError::NotFound(msg) => ApiError::NotFound(msg),
            PersistenceError::AlreadyExists(msg) => ApiError::Conflict(msg),
            PersistenceError::InvalidInput(msg) => ApiError::BadRequest(msg),
            _ => ApiError::InternalServerError(err.to_string()),
        }
    }
}

/// Implements the conversion from `ApiError` into an `axum` response.
///
/// This is the central point for mapping internal application errors to
/// user-facing HTTP responses.
impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, error_message) = match self {
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()),
            ApiError::NotFound(message) => (StatusCode::NOT_FOUND, message),
            ApiError::Conflict(message) => (StatusCode::CONFLICT, message),
            ApiError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            ApiError::InternalServerError(err) => {
                // Log the detailed error for debugging purposes.
                tracing::error!("Internal server error: {}", err);
                // Return a generic error message to the user for security.
                (StatusCode::INTERNAL_SERVER_ERROR, "An internal server error occurred".to_string())
            }
        };

        let body = Json(json!({
            "error": error_message,
        }));

        (status, body).into_response()
    }
}
