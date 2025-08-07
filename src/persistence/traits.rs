//! This module contains the state management logic for the Argus application.

use crate::models::monitor::Monitor;
use async_trait::async_trait;
#[cfg(test)]
use mockall::automock;

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
        monitors: Vec<Monitor>,
    ) -> Result<(), sqlx::Error>;

    /// Clears all monitors for a specific network.
    async fn clear_monitors(&self, network_id: &str) -> Result<(), sqlx::Error>;
}
