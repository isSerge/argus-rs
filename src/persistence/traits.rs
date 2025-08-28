//! This module contains the state management logic for the Argus application.

use async_trait::async_trait;
#[cfg(test)]
use mockall::automock;
use serde::{Serialize, de::DeserializeOwned};

use crate::models::{
    monitor::{Monitor, MonitorConfig},
    notifier::NotifierConfig,
};

/// Represents the state management interface for the Argus application.
#[cfg_attr(test, automock)]
#[async_trait]
pub trait StateRepository: Send + Sync {
    /// Retrieves the last processed block number for a given network.
    async fn get_last_processed_block(&self, network_id: &str) -> Result<Option<u64>, sqlx::Error>;
    /// Sets the last processed block number for a given network.
    async fn set_last_processed_block(
        &self,
        network_id: &str,
        block_number: u64,
    ) -> Result<(), sqlx::Error>;

    /// Performs any necessary cleanup operations before shutdown.
    async fn cleanup(&self) -> Result<(), sqlx::Error>;

    /// Ensures all pending writes are flushed to disk.
    async fn flush(&self) -> Result<(), sqlx::Error>;

    /// Saves emergency state during shutdown (e.g., partial progress).
    async fn save_emergency_state(
        &self,
        network_id: &str,
        block_number: u64,
        note: &str,
    ) -> Result<(), sqlx::Error>;

    // Monitor management operations:
    /// Retrieves all monitors for a specific network.
    async fn get_monitors(&self, network_id: &str) -> Result<Vec<Monitor>, sqlx::Error>;

    /// Adds multiple monitors for a specific network.
    async fn add_monitors(
        &self,
        network_id: &str,
        monitors: Vec<MonitorConfig>,
    ) -> Result<(), sqlx::Error>;

    /// Clears all monitors for a specific network.
    async fn clear_monitors(&self, network_id: &str) -> Result<(), sqlx::Error>;

    // Notifier management operations:
    /// Retrieves all notifiers for a specific network.
    async fn get_notifiers(&self, network_id: &str) -> Result<Vec<NotifierConfig>, sqlx::Error>;

    /// Adds multiple notifiers for a specific network.
    async fn add_notifiers(
        &self,
        network_id: &str,
        notifiers: Vec<NotifierConfig>,
    ) -> Result<(), sqlx::Error>;

    /// Clears all notifiers for a specific network.
    async fn clear_notifiers(&self, network_id: &str) -> Result<(), sqlx::Error>;
}

/// Represents a generic state management interface for JSON-serializable
/// objects.
#[cfg_attr(test, automock)]
#[async_trait]
pub trait GenericStateRepository: Send + Sync {
    /// Retrieves a JSON-serializable state object by its key.
    async fn get_json_state<T: DeserializeOwned + Send + Sync + 'static>(
        &self,
        key: &str,
    ) -> Result<Option<T>, sqlx::Error>;

    /// Sets or updates a JSON-serializable state object by its key.
    async fn set_json_state<T: Serialize + Send + Sync + 'static>(
        &self,
        key: &str,
        value: &T,
    ) -> Result<(), sqlx::Error>;
}
