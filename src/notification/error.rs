//! Error types for the notification service.

use thiserror::Error;

use crate::{http_client::HttpClientPoolError, notification::template::TemplateServiceError};

/// Defines the possible errors that can occur within the notification service.
#[derive(Debug, Error)]
pub enum NotificationError {
    /// An error related to invalid or missing configuration.
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// An error that occurs when failing to deserialize the trigger
    /// configuration from a string.
    #[error("Failed to deserialize trigger configuration: {0}")]
    DeserializationError(#[from] serde_json::Error),

    /// An error during the execution of a notification.
    #[error("Execution error: {0}")]
    ExecutionError(String),

    /// An error indicating that the notification failed to be sent.
    #[error("Notification failed: {0}")]
    NotifyFailed(String),

    /// An internal error that should not occur under normal circumstances.
    #[error("Internal error: {0}")]
    InternalError(String),

    /// An error originating from the HTTP client pool.
    #[error("HTTP client error")]
    HttpClientError(#[from] HttpClientPoolError),

    /// An error from the underlying `reqwest` or `reqwest_middleware`
    /// libraries.
    #[error("Request error: {0}")]
    RequestError(#[from] reqwest_middleware::Error),

    /// An error related to the template rendering process.
    #[error("Template rendering error: {0}")]
    TemplateError(#[from] TemplateServiceError),
}
