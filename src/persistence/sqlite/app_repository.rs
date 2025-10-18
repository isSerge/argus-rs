//! Implementation of the AppRepository trait for SqliteStateRepository

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Utc};

use crate::{
    models::{
        abi::Abi,
        action::ActionConfig,
        monitor::{Monitor, MonitorConfig},
    },
    persistence::{error::PersistenceError, sqlite::SqliteStateRepository, traits::AppRepository},
};

// Helper struct for mapping from the database row
#[derive(sqlx::FromRow)]
struct MonitorRow {
    monitor_id: i64,
    name: String,
    network: String,
    address: Option<String>,
    abi: Option<String>,
    filter_script: String,
    actions: String,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
}

// Helper struct for mapping from the database row
#[derive(sqlx::FromRow)]
struct ActionRow {
    action_id: i64,
    name: String,
    config: String,
}

// Helper struct for mapping ABI from the database row
#[derive(sqlx::FromRow)]
struct AbiRow {
    name: String,
    abi_content: String,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
}

#[async_trait]
impl AppRepository for SqliteStateRepository {
    /// Retrieves the last processed block number for a given network.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn get_last_processed_block(
        &self,
        network_id: &str,
    ) -> Result<Option<u64>, PersistenceError> {
        tracing::debug!(network_id, "Querying for last processed block.");

        let result = self
            .execute_query_with_error_handling(
                "query last processed block",
                sqlx::query!(
                    "SELECT block_number FROM processed_blocks WHERE network_id = ?",
                    network_id
                )
                .fetch_optional(&self.pool),
            )
            .await?;

        match result {
            Some(record) => {
                let block_number: i64 = record.block_number;
                match block_number.try_into() {
                    Ok(block_number_u64) => {
                        tracing::debug!(
                            network_id,
                            block_number = block_number_u64,
                            "Last processed block found."
                        );
                        Ok(Some(block_number_u64))
                    }
                    Err(error) => {
                        tracing::error!(error = %error, network_id, "Failed to convert block_number from i64 to u64.");
                        Err(PersistenceError::OperationFailed(error.to_string()))
                    }
                }
            }
            None => {
                tracing::debug!(network_id, "No last processed block found.");
                Ok(None)
            }
        }
    }

    /// Sets the last processed block number for a given network.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn set_last_processed_block(
        &self,
        network_id: &str,
        block_number: u64,
    ) -> Result<(), PersistenceError> {
        tracing::debug!(network_id, block_number, "Attempting to set last processed block.");

        let block_number_i64 = i64::try_from(block_number).map_err(|error| {
            tracing::error!(error = %error, block_number, "Failed to convert block_number to i64 for database insertion.");
            PersistenceError::InvalidInput(error.to_string())
        })?;

        self.execute_query_with_error_handling(
            "set last processed block",
            sqlx::query!(
                "INSERT OR REPLACE INTO processed_blocks (network_id, block_number) VALUES (?, ?)",
                network_id,
                block_number_i64
            )
            .execute(&self.pool),
        )
        .await?;

        tracing::info!(network_id, block_number, "Last processed block set successfully.");
        Ok(())
    }

    /// Performs any necessary cleanup operations before shutdown.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn cleanup(&self) -> Result<(), PersistenceError> {
        tracing::debug!("Performing state repository cleanup.");

        // Force a checkpoint to ensure all WAL data is written to the main database
        // file
        self.checkpoint_wal("TRUNCATE").await?;

        tracing::debug!("State repository cleanup completed.");
        Ok(())
    }

    /// Ensures all pending writes are flushed to disk.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn flush(&self) -> Result<(), PersistenceError> {
        tracing::debug!("Flushing pending writes to disk.");

        // Temporarily set synchronous mode to FULL for maximum durability
        self.set_synchronous_mode("FULL").await?;

        // Force a checkpoint to flush WAL to main database
        self.checkpoint_wal("TRUNCATE").await?;

        // Revert synchronous mode to NORMAL for better performance during normal
        // operations
        self.set_synchronous_mode("NORMAL").await?;

        tracing::debug!("Pending writes flushed successfully.");
        Ok(())
    }

    /// Saves emergency state during shutdown (e.g., partial progress).
    #[tracing::instrument(skip(self), level = "debug")]
    async fn save_emergency_state(
        &self,
        network_id: &str,
        block_number: u64,
        note: &str,
    ) -> Result<(), PersistenceError> {
        tracing::warn!(
            network_id = %network_id,
            block_number = %block_number,
            note = %note,
            "Saving emergency state during shutdown."
        );

        // Save the current state and flush to ensure it's persisted
        self.set_last_processed_block(network_id, block_number).await?;
        self.flush().await?;

        tracing::info!(
            network_id = %network_id,
            block_number = %block_number,
            note = %note,
            "Emergency state saved and flushed successfully."
        );

        Ok(())
    }

    // Monitor management operations

    /// Retrieves all monitors for a specific network.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn get_monitors(&self, network_id: &str) -> Result<Vec<Monitor>, PersistenceError> {
        tracing::debug!(network_id, "Querying for monitors.");

        let monitor_rows = self
            .execute_query_with_error_handling("query monitors", async {
                sqlx::query_as!(
                    MonitorRow,
                    r#"
                SELECT 
                    monitor_id as "monitor_id!", 
                    name, 
                    network, 
                    address, 
                    abi, 
                    filter_script, 
                    actions,
                    created_at as "created_at!", 
                    updated_at as "updated_at!"
                FROM monitors 
                WHERE network = ?
                "#,
                    network_id
                )
                .fetch_all(&self.pool)
                .await
            })
            .await?;

        let monitors = monitor_rows
            .into_iter()
            .map(|row| {
                let actions: Vec<String> = serde_json::from_str(&row.actions)
                    .map_err(|e| PersistenceError::SerializationError(e.to_string()))?;

                let created_at = DateTime::<Utc>::from_naive_utc_and_offset(row.created_at, Utc);
                let updated_at = DateTime::<Utc>::from_naive_utc_and_offset(row.updated_at, Utc);

                Ok(Monitor {
                    id: row.monitor_id,
                    name: row.name,
                    network: row.network,
                    address: row.address,
                    abi: row.abi,
                    filter_script: row.filter_script,
                    actions,
                    created_at,
                    updated_at,
                })
            })
            .collect::<Result<Vec<_>, PersistenceError>>()?;

        tracing::debug!(
            network_id,
            monitor_count = monitors.len(),
            "Monitors retrieved successfully."
        );
        Ok(monitors)
    }

    /// Retrieves a specific monitor by its ID for a given network.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn get_monitor_by_id(
        &self,
        network_id: &str,
        monitor_id: &str,
    ) -> Result<Option<Monitor>, PersistenceError> {
        tracing::debug!(network_id, monitor_id, "Querying for monitor by ID.");

        let monitor_id_num: i64 = monitor_id.parse().map_err(|e| {
            let msg = format!("Invalid monitor_id '{}': {}", monitor_id, e);
            tracing::error!(error = %e, monitor_id, "Failed to parse monitor_id.");
            PersistenceError::InvalidInput(msg)
        })?;

        let monitor_row = self
            .execute_query_with_error_handling("query monitor by id", async {
                sqlx::query_as!(
                    MonitorRow,
                    r#"
                SELECT 
                    monitor_id as "monitor_id!", 
                    name, 
                    network, 
                    address, 
                    abi, 
                    filter_script, 
                    actions,
                    created_at as "created_at!", 
                    updated_at as "updated_at!"
                FROM monitors 
                WHERE network = ? AND monitor_id = ?
                "#,
                    network_id,
                    monitor_id_num
                )
                .fetch_optional(&self.pool)
                .await
            })
            .await?;

        if let Some(row) = monitor_row {
            let actions: Vec<String> = serde_json::from_str(&row.actions)
                .map_err(|e| PersistenceError::SerializationError(e.to_string()))?;

            let created_at = DateTime::<Utc>::from_naive_utc_and_offset(row.created_at, Utc);
            let updated_at = DateTime::<Utc>::from_naive_utc_and_offset(row.updated_at, Utc);

            let monitor = Monitor {
                id: row.monitor_id,
                name: row.name,
                network: row.network,
                address: row.address,
                abi: row.abi,
                filter_script: row.filter_script,
                actions,
                created_at,
                updated_at,
            };

            tracing::debug!(network_id, monitor_id, "Monitor found.");
            Ok(Some(monitor))
        } else {
            tracing::debug!(network_id, monitor_id, "No monitor found with given ID.");
            Ok(None)
        }
    }

    /// Adds multiple monitors for a specific network.
    #[tracing::instrument(skip(self, monitors), level = "debug")]
    async fn add_monitors(
        &self,
        network_id: &str,
        monitors: Vec<MonitorConfig>,
    ) -> Result<(), PersistenceError> {
        tracing::debug!(network_id, monitor_count = monitors.len(), "Adding monitors.");

        // Validate that all monitors belong to the correct network
        for monitor in &monitors {
            if monitor.network != network_id {
                let msg = format!(
                    "Monitor '{}' has network '{}' but expected '{}'",
                    monitor.name, monitor.network, network_id
                );
                tracing::error!(
                    expected_network = network_id,
                    actual_network = monitor.network,
                    monitor_name = monitor.name,
                    "Monitor network mismatch."
                );
                return Err(PersistenceError::InvalidInput(msg));
            }
        }

        // Insert monitors in a transaction for atomicity
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| PersistenceError::OperationFailed(e.to_string()))?;

        for monitor in monitors {
            let actions_str = serde_json::to_string(&monitor.actions)
                .map_err(|e| PersistenceError::SerializationError(e.to_string()))?;

            sqlx::query!(
                "INSERT INTO monitors (name, network, address, abi, filter_script, actions) \
                 VALUES (?, ?, ?, ?, ?, ?)",
                monitor.name,
                monitor.network,
                monitor.address,
                monitor.abi,
                monitor.filter_script,
                actions_str,
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| PersistenceError::OperationFailed(e.to_string()))?;
        }

        tx.commit().await.map_err(|e| PersistenceError::OperationFailed(e.to_string()))?;

        tracing::info!(network_id, "Monitors added successfully.");
        Ok(())
    }

    /// Clears all monitors for a specific network.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn clear_monitors(&self, network_id: &str) -> Result<(), PersistenceError> {
        tracing::debug!(network_id, "Clearing monitors.");

        let result = self
            .execute_query_with_error_handling(
                "clear monitors",
                sqlx::query!("DELETE FROM monitors WHERE network = ?", network_id)
                    .execute(&self.pool),
            )
            .await?;

        let deleted_count = result.rows_affected();
        tracing::info!(network_id, deleted_count, "Monitors cleared successfully.");
        Ok(())
    }

    // Action management operations

    /// Retrieves all actions for a specific network.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn get_actions(&self, network_id: &str) -> Result<Vec<ActionConfig>, PersistenceError> {
        tracing::debug!(network_id, "Querying for actions.");

        let action_rows = self
            .execute_query_with_error_handling(
                "query actions",
                sqlx::query_as!(
                    ActionRow,
                    "SELECT action_id, name, config FROM actions WHERE network_id = ?",
                    network_id
                )
                .fetch_all(&self.pool),
            )
            .await?;

        let actions = action_rows
            .into_iter()
            .map(|row| {
                let mut action: ActionConfig = serde_json::from_str(&row.config)
                    .map_err(|e| PersistenceError::SerializationError(e.to_string()))?;
                action.id = Some(row.action_id);
                action.name = row.name;
                Ok(action)
            })
            .collect::<Result<Vec<_>, PersistenceError>>()?;

        tracing::debug!(
            network_id,
            action_count = actions.len(),
            "actions retrieved successfully."
        );
        Ok(actions)
    }

    /// Retrieves a specific action by its ID for a given network.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn get_action_by_id(
        &self,
        network_id: &str,
        action_id: &str,
    ) -> Result<Option<ActionConfig>, PersistenceError> {
        tracing::debug!(network_id, action_id, "Querying for action by ID.");

        let action_id_num: i64 = action_id.parse().map_err(|e| {
            let msg = format!("Invalid action_id '{}': {}", action_id, e);
            tracing::error!(error = %e, action_id, "Failed to parse action_id.");
            PersistenceError::InvalidInput(msg)
        })?;

        let action_row = self
            .execute_query_with_error_handling(
                "query action by id",
                sqlx::query_as!(
                    ActionRow,
                    "SELECT action_id, name, config FROM actions WHERE network_id = ? AND \
                     action_id = ?",
                    network_id,
                    action_id_num
                )
                .fetch_optional(&self.pool),
            )
            .await?;

        if let Some(row) = action_row {
            let mut action: ActionConfig = serde_json::from_str(&row.config)
                .map_err(|e| PersistenceError::SerializationError(e.to_string()))?;
            action.id = Some(row.action_id);
            action.name = row.name;
            tracing::debug!(network_id, action_id, "Action found.");
            Ok(Some(action))
        } else {
            tracing::debug!(network_id, action_id, "No action found with given ID.");
            Ok(None)
        }
    }

    /// Adds multiple actions for a specific network.
    #[tracing::instrument(skip(self, actions), level = "debug")]
    async fn add_actions(
        &self,
        network_id: &str,
        actions: Vec<ActionConfig>,
    ) -> Result<(), PersistenceError> {
        tracing::debug!(network_id, action_count = actions.len(), "Adding actions.");

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| PersistenceError::OperationFailed(e.to_string()))?;

        for action in actions {
            let config = serde_json::to_string(&action)
                .map_err(|e| PersistenceError::SerializationError(e.to_string()))?;

            sqlx::query!(
                "INSERT INTO actions (name, network_id, config) VALUES (?, ?, ?)",
                action.name,
                network_id,
                config
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| PersistenceError::OperationFailed(e.to_string()))?;
        }

        tx.commit().await.map_err(|e| PersistenceError::OperationFailed(e.to_string()))?;

        tracing::info!(network_id, "actions added successfully.");
        Ok(())
    }

    /// Clears all actions for a specific network.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn clear_actions(&self, network_id: &str) -> Result<(), PersistenceError> {
        tracing::debug!(network_id, "Clearing actions.");

        let result = self
            .execute_query_with_error_handling(
                "clear actions",
                sqlx::query!("DELETE FROM actions WHERE network_id = ?", network_id)
                    .execute(&self.pool),
            )
            .await?;

        let deleted_count = result.rows_affected();
        tracing::info!(network_id, deleted_count, "actions cleared successfully.");
        Ok(())
    }

    // ABI management operations

    /// Retrieves all ABIs from the database.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn get_abis(&self) -> Result<Vec<Abi>, PersistenceError> {
        tracing::debug!("Querying for all ABIs.");

        let abi_rows = self
            .execute_query_with_error_handling(
                "query ABIs",
                sqlx::query_as!(
                    AbiRow,
                    r#"
                SELECT 
                    name, 
                    abi_content, 
                    created_at as "created_at!", 
                    updated_at as "updated_at!"
                FROM abis
                ORDER BY name
                "#
                )
                .fetch_all(&self.pool),
            )
            .await?;

        let abis = abi_rows
            .into_iter()
            .map(|row| {
                let created_at = DateTime::<Utc>::from_naive_utc_and_offset(row.created_at, Utc);
                let updated_at = DateTime::<Utc>::from_naive_utc_and_offset(row.updated_at, Utc);
                Abi {
                    name: row.name,
                    abi_content: row.abi_content,
                    created_at,
                    updated_at,
                }
            })
            .collect();

        tracing::debug!(abi_count = abis.len(), "ABIs retrieved successfully.");
        Ok(abis)
    }

    /// Retrieves a specific ABI by its name.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn get_abi_by_name(&self, name: &str) -> Result<Option<Abi>, PersistenceError> {
        tracing::debug!(name, "Querying for ABI by name.");

        let abi_row = self
            .execute_query_with_error_handling(
                "query ABI by name",
                sqlx::query_as!(
                    AbiRow,
                    r#"
                SELECT 
                    name, 
                    abi_content, 
                    created_at as "created_at!", 
                    updated_at as "updated_at!"
                FROM abis 
                WHERE name = ?
                "#,
                    name
                )
                .fetch_optional(&self.pool),
            )
            .await?;

        if let Some(row) = abi_row {
            let created_at = DateTime::<Utc>::from_naive_utc_and_offset(row.created_at, Utc);
            let updated_at = DateTime::<Utc>::from_naive_utc_and_offset(row.updated_at, Utc);
            let abi = Abi {
                name: row.name,
                abi_content: row.abi_content,
                created_at,
                updated_at,
            };
            tracing::debug!(name, "ABI found.");
            Ok(Some(abi))
        } else {
            tracing::debug!(name, "No ABI found with given name.");
            Ok(None)
        }
    }

    /// Adds a new ABI to the database.
    #[tracing::instrument(skip(self, abi_content), level = "debug")]
    async fn add_abi(&self, name: &str, abi_content: &str) -> Result<(), PersistenceError> {
        tracing::debug!(name, "Adding new ABI.");

        // Validate that the ABI content is valid JSON
        serde_json::from_str::<serde_json::Value>(abi_content).map_err(|e| {
            tracing::error!(name, error = %e, "Invalid ABI JSON content.");
            PersistenceError::InvalidInput(format!("Invalid ABI JSON: {}", e))
        })?;

        // Check if ABI already exists
        let existing = self.get_abi_by_name(name).await?;
        if existing.is_some() {
            tracing::warn!(name, "ABI with this name already exists.");
            return Err(PersistenceError::AlreadyExists(format!(
                "ABI with name '{}' already exists",
                name
            )));
        }

        self.execute_query_with_error_handling(
            "add ABI",
            sqlx::query!(
                "INSERT INTO abis (name, abi_content) VALUES (?, ?)",
                name,
                abi_content
            )
            .execute(&self.pool),
        )
        .await?;

        tracing::info!(name, "ABI added successfully.");
        Ok(())
    }

    /// Deletes an ABI by its name.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn delete_abi(&self, name: &str) -> Result<(), PersistenceError> {
        tracing::debug!(name, "Deleting ABI.");

        // Check if ABI is in use by any monitors
        if self.is_abi_in_use(name).await? {
            tracing::warn!(name, "Cannot delete ABI that is in use by monitors.");
            return Err(PersistenceError::InvalidInput(format!(
                "Cannot delete ABI '{}' as it is currently in use by one or more monitors",
                name
            )));
        }

        let result = self
            .execute_query_with_error_handling(
                "delete ABI",
                sqlx::query!("DELETE FROM abis WHERE name = ?", name).execute(&self.pool),
            )
            .await?;

        if result.rows_affected() == 0 {
            tracing::warn!(name, "No ABI found with given name to delete.");
            return Err(PersistenceError::NotFound(format!("ABI '{}' not found", name)));
        }

        tracing::info!(name, "ABI deleted successfully.");
        Ok(())
    }

    /// Checks if an ABI is currently in use by any monitors.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn is_abi_in_use(&self, name: &str) -> Result<bool, PersistenceError> {
        tracing::debug!(name, "Checking if ABI is in use.");

        let count = self
            .execute_query_with_error_handling(
                "check ABI usage",
                sqlx::query!("SELECT COUNT(*) as count FROM monitors WHERE abi = ?", name)
                    .fetch_one(&self.pool),
            )
            .await?;

        let is_in_use = count.count > 0;
        tracing::debug!(name, is_in_use, "ABI usage check completed.");
        Ok(is_in_use)
    }
}
