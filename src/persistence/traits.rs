//! This module contains the persistence traits for the Argus application.

use async_trait::async_trait;
#[cfg(test)]
use mockall::automock;
use serde::{Serialize, de::DeserializeOwned};

use super::error::PersistenceError;
use crate::models::{
    monitor::{Monitor, MonitorConfig},
    notifier::NotifierConfig,
};

/// Represents the application's persistence layer interface.
#[cfg_attr(test, automock)]
#[async_trait]
pub trait AppRepository: Send + Sync {
    /// Retrieves the last processed block number for a given network.
    async fn get_last_processed_block(
        &self,
        network_id: &str,
    ) -> Result<Option<u64>, PersistenceError>;
    /// Sets the last processed block number for a given network.
    async fn set_last_processed_block(
        &self,
        network_id: &str,
        block_number: u64,
    ) -> Result<(), PersistenceError>;

    /// Performs any necessary cleanup operations before shutdown.
    async fn cleanup(&self) -> Result<(), PersistenceError>;

    /// Ensures all pending writes are flushed to disk.
    async fn flush(&self) -> Result<(), PersistenceError>;

    /// Saves emergency state during shutdown (e.g., partial progress).
    async fn save_emergency_state(
        &self,
        network_id: &str,
        block_number: u64,
        note: &str,
    ) -> Result<(), PersistenceError>;

    // Monitor management operations:
    /// Retrieves all monitors for a specific network.
    async fn get_monitors(&self, network_id: &str) -> Result<Vec<Monitor>, PersistenceError>;

    /// Adds multiple monitors for a specific network.
    async fn add_monitors(
        &self,
        network_id: &str,
        monitors: Vec<MonitorConfig>,
    ) -> Result<(), PersistenceError>;

    /// Clears all monitors for a specific network.
    async fn clear_monitors(&self, network_id: &str) -> Result<(), PersistenceError>;

    // Notifier management operations:
    /// Retrieves all notifiers for a specific network.
    async fn get_notifiers(
        &self,
        network_id: &str,
    ) -> Result<Vec<NotifierConfig>, PersistenceError>;

    /// Adds multiple notifiers for a specific network.
    async fn add_notifiers(
        &self,
        network_id: &str,
        notifiers: Vec<NotifierConfig>,
    ) -> Result<(), PersistenceError>;

    /// Clears all notifiers for a specific network.
    async fn clear_notifiers(&self, network_id: &str) -> Result<(), PersistenceError>;
}

/// Represents a generic key-value store for JSON-serializable objects.
#[cfg_attr(test, automock)]
#[async_trait]
pub trait KeyValueStore: Send + Sync {
    /// Retrieves a JSON-serializable state object by its key.
    async fn get_json_state<T: DeserializeOwned + Send + Sync + 'static>(
        &self,
        key: &str,
    ) -> Result<Option<T>, PersistenceError>;

    /// Sets or updates a JSON-serializable state object by its key.
    async fn set_json_state<T: Serialize + Send + Sync + 'static>(
        &self,
        key: &str,
        value: &T,
    ) -> Result<(), PersistenceError>;

    /// Retrieves all JSON-serializable state objects matching a key prefix.
    async fn get_all_json_states_by_prefix<T: DeserializeOwned + Send + Sync + 'static>(
        &self,
        prefix: &str,
    ) -> Result<Vec<(String, T)>, PersistenceError>;
}
