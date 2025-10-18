//! This module contains the error types for the persistence layer.

use thiserror::Error;

/// Errors that can occur in the persistence layer.
#[derive(Error, Debug)]
pub enum PersistenceError {
    /// A general error occurred during a data store operation.
    #[error("A data store operation failed: {0}")]
    OperationFailed(String),

    /// The requested item was not found in the data store.
    #[error("The requested item was not found: {0}")]
    NotFound(String),

    /// An error occurred during serialization or deserialization.
    #[error("Failed to serialize or deserialize data: {0}")]
    SerializationError(String),

    /// An error occurred during a database migration.
    #[error("A data migration failed: {0}")]
    MigrationError(String),

    /// An invalid configuration or input was provided.
    #[error("An invalid configuration or input was provided: {0}")]
    InvalidInput(String),

    /// The item already exists in the data store.
    #[error("Item already exists: {0}")]
    AlreadyExists(String),
}
