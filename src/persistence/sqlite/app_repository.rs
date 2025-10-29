//! Implementation of the AppRepository trait for SqliteStateRepository

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Utc};

use crate::{
    models::{
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
    abi_name: Option<String>,
    filter_script: String,
    actions: String,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
}

// Helper struct for mapping from the database row
#[derive(sqlx::FromRow)]
struct ActionRow {
    action_id: Option<i64>,
    name: String,
    config: String,
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
                    abi_name, 
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
                    abi_name: row.abi_name,
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
                    abi_name, 
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
                abi_name: row.abi_name,
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
                "INSERT INTO monitors (name, network, address, abi_name, filter_script, actions) \
                 VALUES (?, ?, ?, ?, ?, ?)",
                monitor.name,
                monitor.network,
                monitor.address,
                monitor.abi_name,
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

    /// Creates a new ABI.
    #[tracing::instrument(skip(self, abi), level = "debug")]
    async fn create_abi(&self, name: &str, abi: &str) -> Result<(), PersistenceError> {
        tracing::debug!(name, "Creating ABI.");

        self.execute_query_with_error_handling(
            "create abi",
            sqlx::query!("INSERT INTO abis (name, abi) VALUES (?, ?)", name, abi)
                .execute(&self.pool),
        )
        .await?;

        tracing::info!(name, "ABI created successfully.");
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
                action.id = row.action_id;
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
        action_id: i64,
    ) -> Result<Option<ActionConfig>, PersistenceError> {
        tracing::debug!(network_id, action_id, "Querying for action by ID.");

        let action_row = self
            .execute_query_with_error_handling(
                "query action by id",
                sqlx::query_as!(
                    ActionRow,
                    "SELECT action_id, name, config FROM actions WHERE network_id = ? AND \
                     action_id = ?",
                    network_id,
                    action_id
                )
                .fetch_optional(&self.pool),
            )
            .await?;

        if let Some(row) = action_row {
            let mut action: ActionConfig = serde_json::from_str(&row.config)
                .map_err(|e| PersistenceError::SerializationError(e.to_string()))?;
            action.id = row.action_id;
            action.name = row.name;
            tracing::debug!(network_id, action_id, "Action found.");
            Ok(Some(action))
        } else {
            tracing::debug!(network_id, action_id, "No action found with given ID.");
            Ok(None)
        }
    }

    /// Retrieves a specific action by its name for a given network.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn get_action_by_name(
        &self,
        network_id: &str,
        name: &str,
    ) -> Result<Option<ActionConfig>, PersistenceError> {
        tracing::debug!(network_id, name, "Querying for action by name.");

        let action_row = self
            .execute_query_with_error_handling(
                "query action by name",
                sqlx::query_as!(
                    ActionRow,
                    "SELECT action_id, name, config FROM actions WHERE network_id = ? AND name = ?",
                    network_id,
                    name
                )
                .fetch_optional(&self.pool),
            )
            .await?;

        if let Some(row) = action_row {
            let mut action: ActionConfig = serde_json::from_str(&row.config)
                .map_err(|e| PersistenceError::SerializationError(e.to_string()))?;
            action.id = row.action_id;
            action.name = row.name;
            tracing::debug!(network_id, name, "Action found.");
            Ok(Some(action))
        } else {
            tracing::debug!(network_id, name, "No action found with given name.");
            Ok(None)
        }
    }

    /// Creates a new action for a specific network.
    #[tracing::instrument(skip(self, action), level = "debug")]
    async fn create_action(
        &self,
        network_id: &str,
        action: ActionConfig,
    ) -> Result<ActionConfig, PersistenceError> {
        tracing::debug!(network_id, action_name = action.name, "Creating action.");

        let config = serde_json::to_string(&action)
            .map_err(|e| PersistenceError::SerializationError(e.to_string()))?;

        let result = self
            .execute_query_with_error_handling(
                "create action",
                sqlx::query!(
                    "INSERT INTO actions (name, network_id, config) VALUES (?, ?, ?)",
                    action.name,
                    network_id,
                    config
                )
                .execute(&self.pool),
            )
            .await?;

        let new_id = result.last_insert_rowid();
        let mut new_action = action;
        new_action.id = Some(new_id);

        tracing::info!(
            network_id,
            action_name = new_action.name,
            action_id = new_id,
            "Action created successfully."
        );
        Ok(new_action)
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

    /// Updates an existing action for a specific network.
    #[tracing::instrument(skip(self, action), level = "debug")]
    async fn update_action(
        &self,
        network_id: &str,
        action: ActionConfig,
    ) -> Result<ActionConfig, PersistenceError> {
        let action_id = action.id.ok_or_else(|| {
            PersistenceError::InvalidInput("Action ID is required for update".to_string())
        })?;
        tracing::debug!(network_id, action_id, "Updating action.");

        let config = serde_json::to_string(&action)
            .map_err(|e| PersistenceError::SerializationError(e.to_string()))?;

        let result = self
            .execute_query_with_error_handling(
                "update action",
                sqlx::query!(
                    "UPDATE actions SET name = ?, config = ? WHERE network_id = ? AND action_id = \
                     ?",
                    action.name,
                    config,
                    network_id,
                    action_id
                )
                .execute(&self.pool),
            )
            .await?;

        if result.rows_affected() == 0 {
            return Err(PersistenceError::NotFound);
        }

        tracing::info!(network_id, action_id, "Action updated successfully.");
        Ok(action)
    }

    /// Deletes an action by its ID for a specific network.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn delete_action(
        &self,
        network_id: &str,
        action_id: i64,
    ) -> Result<(), PersistenceError> {
        tracing::debug!(network_id, action_id, "Deleting action.");

        let result = self
            .execute_query_with_error_handling(
                "delete action",
                sqlx::query!(
                    "DELETE FROM actions WHERE network_id = ? AND action_id = ?",
                    network_id,
                    action_id
                )
                .execute(&self.pool),
            )
            .await?;

        if result.rows_affected() == 0 {
            return Err(PersistenceError::NotFound);
        }

        tracing::info!(network_id, action_id, "Action deleted successfully.");
        Ok(())
    }

    /// Retrieves all monitors that are associated with a specific action.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn get_monitors_by_action_id(
        &self,
        network_id: &str,
        action_id: i64,
    ) -> Result<Vec<MonitorConfig>, PersistenceError> {
        tracing::debug!(network_id, action_id, "Querying for monitors by action ID.");

        let action_name = self
            .get_action_by_id(network_id, action_id)
            .await?
            .ok_or(PersistenceError::NotFound)?
            .name;

        // Escape LIKE wildcards to prevent incorrect matching.
        let escaped_action_name = action_name.replace('%', "\\%").replace('_', "\\_");
        let like_clause = format!("%\"{}\"%", escaped_action_name);

        let monitor_rows = self
            .execute_query_with_error_handling("query monitors by action id", async {
                sqlx::query_as!(
                    MonitorRow,
                    r#"
                    SELECT 
                        monitor_id as "monitor_id!", 
                        name, 
                        network, 
                        address, 
                        abi_name, 
                        filter_script, 
                        actions,
                        created_at as "created_at!", 
                        updated_at as "updated_at!"
                    FROM monitors 
                    WHERE network = ? AND actions LIKE ? ESCAPE '\'
                    "#,
                    network_id,
                    like_clause
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

                Ok(MonitorConfig {
                    name: row.name,
                    network: row.network,
                    address: row.address,
                    abi_name: row.abi_name,
                    filter_script: row.filter_script,
                    actions,
                })
            })
            .collect::<Result<Vec<_>, PersistenceError>>()?;

        tracing::debug!(
            network_id,
            action_id,
            monitor_count = monitors.len(),
            "Monitors retrieved successfully."
        );
        Ok(monitors)
    }
}
