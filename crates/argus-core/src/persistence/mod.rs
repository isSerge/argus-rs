//! Persistence layer types and traits.

pub mod error;
pub mod traits;

pub use error::PersistenceError;
pub use traits::{AppRepository, KeyValueStore, OutboxItem};
