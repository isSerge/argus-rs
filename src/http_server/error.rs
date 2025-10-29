//! Defines the custom `ApiError` type for the HTTP server.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde_json::json;

use crate::{action::validator::ActionValidationError, persistence::error::PersistenceError};

/// A custom error type for the API that can be converted into an HTTP response.
pub enum ApiError {
    /// Represents an unauthorized request.
    Unauthorized,

    /// Represents a resource that could not be found.
    NotFound(String),

    /// Represents a validation error for an unprocessable entity.
    UnprocessableEntity(String),

    /// Represents a conflict, e.g., a resource that already exists.
    Conflict(String),

    /// Represents a conflict where an action is in use by monitors.
    ActionInUse(Vec<String>),

    /// Represents a conflict where an ABI is in use by monitors.
    AbiInUse(Vec<String>),

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
            PersistenceError::NotFound => ApiError::NotFound("Resource not found".to_string()),
            _ => ApiError::InternalServerError(err.to_string()),
        }
    }
}

impl From<ActionValidationError> for ApiError {
    fn from(err: ActionValidationError) -> Self {
        match err {
            ActionValidationError::Configuration(e) => ApiError::UnprocessableEntity(e.to_string()),
            ActionValidationError::NameConflict(name) =>
                ApiError::Conflict(format!("Action with name '{}' already exists.", name)),
            ActionValidationError::Persistence(PersistenceError::NotFound) =>
                ApiError::NotFound("Action not found".to_string()),
            ActionValidationError::Persistence(e) => ApiError::InternalServerError(e.to_string()),
        }
    }
}

/// Implements the conversion from `ApiError` into an `axum` response.
///
/// This is the central point for mapping internal application errors to
/// user-facing HTTP responses.
impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, body) = match self {
            ApiError::Unauthorized =>
                (StatusCode::UNAUTHORIZED, json!({ "error": "Unauthorized" })),
            ApiError::InternalServerError(err) => {
                tracing::error!("Internal server error: {}", err);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({ "error": "An internal server error occurred" }),
                )
            }
            ApiError::NotFound(message) => (StatusCode::NOT_FOUND, json!({ "error": message })),
            ApiError::UnprocessableEntity(message) =>
                (StatusCode::UNPROCESSABLE_ENTITY, json!({ "error": message })),
            ApiError::Conflict(message) => (StatusCode::CONFLICT, json!({ "error": message })),
            ApiError::ActionInUse(monitors) => (
                StatusCode::CONFLICT,
                json!({
                    "error": "Action is in use and cannot be deleted.",
                    "monitors": monitors,
                }),
            ),
            ApiError::AbiInUse(monitors) => (
                StatusCode::CONFLICT,
                json!({
                    "error": "ABI is in use and cannot be deleted.",
                    "monitors": monitors,
                }),
            ),
        };

        (status, Json(body)).into_response()
    }
}
