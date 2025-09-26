//! This module contains the state management logic for the Argus application.

pub mod error;
pub mod sqlite;
pub use sqlite::SqliteStateRepository;
pub mod traits;
