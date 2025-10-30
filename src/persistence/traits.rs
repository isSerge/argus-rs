//! This module contains the persistence traits for the Argus application.

use async_trait::async_trait;
#[cfg(test)]
use mockall::automock;
use serde::{Serialize, de::DeserializeOwned};

use super::error::PersistenceError;
use crate::models::{
    action::ActionConfig,
    monitor::{Monitor, MonitorConfig},
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

    /// Retrieves a specific monitor by its ID for a given network.
    async fn get_monitor_by_id(
        &self,
        network_id: &str,
        monitor_id: &str,
    ) -> Result<Option<Monitor>, PersistenceError>;

    /// Adds multiple monitors for a specific network.
    async fn add_monitors(
        &self,
        network_id: &str,
        monitors: Vec<MonitorConfig>,
    ) -> Result<(), PersistenceError>;

    /// Clears all monitors for a specific network.
    async fn clear_monitors(&self, network_id: &str) -> Result<(), PersistenceError>;

    // ABI management operations:
    /// Creates a new ABI.
    async fn create_abi(&self, name: &str, abi: &str) -> Result<(), PersistenceError>;

    /// Retrieves an ABI by its name.
    async fn get_abi(&self, name: &str) -> Result<Option<String>, PersistenceError>;

    /// Lists all available ABI names.
    async fn list_abis(&self) -> Result<Vec<String>, PersistenceError>;

    /// Deletes an ABI by its name.
    async fn delete_abi(&self, name: &str) -> Result<(), PersistenceError>;

    /// Retrieves all ABIs as a vector of (name, abi_json_string) tuples.
    async fn get_all_abis(&self) -> Result<Vec<(String, String)>, PersistenceError>;

    // Action management operations:
    /// Retrieves all actions for a specific network.
    async fn get_actions(&self, network_id: &str) -> Result<Vec<ActionConfig>, PersistenceError>;

    /// Retrieves a specific action by its ID for a given network.
    async fn get_action_by_id(
        &self,
        network_id: &str,
        action_id: i64,
    ) -> Result<Option<ActionConfig>, PersistenceError>;

    /// Retrieves a specific action by its name for a given network.
    async fn get_action_by_name(
        &self,
        network_id: &str,
        name: &str,
    ) -> Result<Option<ActionConfig>, PersistenceError>;

    /// Creates a new action for a specific network.
    async fn create_action(
        &self,
        network_id: &str,
        action: ActionConfig,
    ) -> Result<ActionConfig, PersistenceError>;

    /// Clears all actions for a specific network.
    async fn clear_actions(&self, network_id: &str) -> Result<(), PersistenceError>;

    /// Updates an existing action for a specific network.
    async fn update_action(
        &self,
        network_id: &str,
        action: ActionConfig,
    ) -> Result<ActionConfig, PersistenceError>;

    /// Deletes an action by its ID for a specific network.
    async fn delete_action(&self, network_id: &str, action_id: i64)
    -> Result<(), PersistenceError>;

    /// Retrieves all monitors that are associated with a specific action.
    async fn get_monitors_by_action_id(
        &self,
        network_id: &str,
        action_id: i64,
    ) -> Result<Vec<crate::models::monitor::MonitorConfig>, PersistenceError>;
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
