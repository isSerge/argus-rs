//! This module provides a concrete implementation of the StateRepository using SQLite.

use super::traits::StateRepository;
use crate::models::monitor::Monitor;
use async_trait::async_trait;
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteRow},
};
use std::str::FromStr;

/// SQL query constants for monitor operations
mod monitor_sql {
    /// Select all monitors for a specific network
    pub const SELECT_MONITORS_BY_NETWORK: &str = "SELECT monitor_id, name, network, address, filter_script, created_at, updated_at FROM monitors WHERE network = ?";

    /// Insert a new monitor
    pub const INSERT_MONITOR: &str =
        "INSERT INTO monitors (name, network, address, filter_script) VALUES (?, ?, ?, ?)";

    /// Delete all monitors for a specific network
    pub const DELETE_MONITORS_BY_NETWORK: &str = "DELETE FROM monitors WHERE network = ?";
}

/// SQL query constants for processed blocks operations  
mod block_sql {
    /// Select last processed block for a network
    pub const SELECT_LAST_PROCESSED_BLOCK: &str =
        "SELECT block_number FROM processed_blocks WHERE network_id = ?";

    /// Insert or replace last processed block
    pub const UPSERT_LAST_PROCESSED_BLOCK: &str =
        "INSERT OR REPLACE INTO processed_blocks (network_id, block_number) VALUES (?, ?)";
}

/// A concrete implementation of the StateRepository using SQLite.
pub struct SqliteStateRepository {
    /// The SQLite connection pool used for database operations.
    pool: SqlitePool,
}

impl SqliteStateRepository {
    /// Creates a new instance of SqliteStateRepository with the provided database URL.
    /// This will create the database file if it does not exist.
    #[tracing::instrument(level = "info")]
    pub async fn new(database_url: &str) -> Result<Self, sqlx::Error> {
        tracing::debug!(database_url, "Attempting to connect to SQLite database.");
        let options = SqliteConnectOptions::from_str(database_url)?.create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await?;
        tracing::info!(database_url, "Successfully connected to SQLite database.");
        Ok(Self { pool })
    }

    /// Runs database migrations.
    #[tracing::instrument(skip(self), level = "info")]
    pub async fn run_migrations(&self) -> Result<(), sqlx::migrate::MigrateError> {
        tracing::debug!("Running database migrations.");
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to run database migrations.");
                e
            })?;
        tracing::info!("Database migrations completed successfully.");
        Ok(())
    }

    /// Gets access to the underlying connection pool for advanced operations.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Closes the connection pool gracefully.
    #[tracing::instrument(skip(self), level = "info")]
    pub async fn close(&self) {
        tracing::debug!("Closing SQLite connection pool.");
        self.pool.close().await;
        tracing::info!("SQLite connection pool closed successfully.");
    }

    /// Internal helper to execute a PRAGMA command with error handling
    async fn execute_pragma(&self, pragma: &str, operation: &str) -> Result<(), sqlx::Error> {
        sqlx::query(pragma)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, pragma = %pragma, operation = %operation, "Failed to execute PRAGMA command.");
                e
            })?;
        Ok(())
    }

    /// Performs a WAL checkpoint with the specified mode
    async fn checkpoint_wal(&self, mode: &str) -> Result<(), sqlx::Error> {
        let allowed_modes = ["PASSIVE", "TRUNCATE", "RESTART", "RESTART_OR_TRUNCATE"];
        if !allowed_modes.contains(&mode) {
            return Err(sqlx::Error::Protocol(format!(
                "Invalid WAL checkpoint mode: {mode}"
            )));
        }
        let pragma = format!("PRAGMA wal_checkpoint({mode})");
        self.execute_pragma(&pragma, &format!("WAL checkpoint {mode}"))
            .await
    }

    /// Sets the synchronous mode
    async fn set_synchronous_mode(&self, mode: &str) -> Result<(), sqlx::Error> {
        let allowed_modes = ["OFF", "NORMAL", "FULL"];
        if !allowed_modes.contains(&mode) {
            return Err(sqlx::Error::Protocol(format!(
                "Invalid synchronous mode: {mode}"
            )));
        }
        let pragma = format!("PRAGMA synchronous = {mode}");
        self.execute_pragma(&pragma, &format!("set synchronous mode to {mode}"))
            .await
    }

    /// Helper to execute database queries with consistent error handling
    async fn execute_query_with_error_handling<F, T>(
        &self,
        operation: &str,
        query_fn: F,
    ) -> Result<T, sqlx::Error>
    where
        F: std::future::Future<Output = Result<T, sqlx::Error>>,
    {
        query_fn.await.map_err(|e| {
            tracing::error!(error = %e, operation = %operation, "Database operation failed.");
            e
        })
    }
}

#[async_trait]
impl StateRepository for SqliteStateRepository {
    /// Retrieves the last processed block number for a given network.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn get_last_processed_block(&self, network_id: &str) -> Result<Option<u64>, sqlx::Error> {
        tracing::debug!(network_id, "Querying for last processed block.");

        let result: Option<SqliteRow> = self
            .execute_query_with_error_handling(
                "query last processed block",
                sqlx::query(block_sql::SELECT_LAST_PROCESSED_BLOCK)
                    .bind(network_id)
                    .fetch_optional(&self.pool),
            )
            .await?;

        match result {
            Some(row) => {
                let block_number: i64 = row.get("block_number");
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
                        Err(sqlx::Error::ColumnDecode {
                            index: "block_number".to_string(),
                            source: Box::new(error),
                        })
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
    ) -> Result<(), sqlx::Error> {
        tracing::debug!(
            network_id,
            block_number,
            "Attempting to set last processed block."
        );

        let block_number_i64 = i64::try_from(block_number).map_err(|error| {
            tracing::error!(error = %error, block_number, "Failed to convert block_number to i64 for database insertion.");
            sqlx::Error::ColumnDecode {
                index: "block_number".to_string(),
                source: Box::new(error),
            }
        })?;

        self.execute_query_with_error_handling(
            "set last processed block",
            sqlx::query(block_sql::UPSERT_LAST_PROCESSED_BLOCK)
                .bind(network_id)
                .bind(block_number_i64)
                .execute(&self.pool),
        )
        .await?;

        tracing::info!(
            network_id,
            block_number,
            "Last processed block set successfully."
        );
        Ok(())
    }

    /// Performs any necessary cleanup operations before shutdown.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn cleanup(&self) -> Result<(), sqlx::Error> {
        tracing::debug!("Performing state repository cleanup.");

        // Force a checkpoint to ensure all WAL data is written to the main database file
        self.checkpoint_wal("TRUNCATE").await?;

        tracing::debug!("State repository cleanup completed.");
        Ok(())
    }

    /// Ensures all pending writes are flushed to disk.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn flush(&self) -> Result<(), sqlx::Error> {
        tracing::debug!("Flushing pending writes to disk.");

        // Temporarily set synchronous mode to FULL for maximum durability
        self.set_synchronous_mode("FULL").await?;

        // Force a checkpoint to flush WAL to main database
        self.checkpoint_wal("TRUNCATE").await?;

        // Revert synchronous mode to NORMAL for better performance during normal operations
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
    ) -> Result<(), sqlx::Error> {
        tracing::warn!(
            network_id = %network_id,
            block_number = %block_number,
            note = %note,
            "Saving emergency state during shutdown."
        );

        // Save the current state and flush to ensure it's persisted
        self.set_last_processed_block(network_id, block_number)
            .await?;
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
    async fn get_monitors(&self, network_id: &str) -> Result<Vec<Monitor>, sqlx::Error> {
        tracing::debug!(network_id, "Querying for monitors.");

        let monitors = self
            .execute_query_with_error_handling(
                "query monitors",
                sqlx::query_as::<_, Monitor>(monitor_sql::SELECT_MONITORS_BY_NETWORK)
                    .bind(network_id)
                    .fetch_all(&self.pool),
            )
            .await?;

        tracing::debug!(
            network_id,
            monitor_count = monitors.len(),
            "Monitors retrieved successfully."
        );
        Ok(monitors)
    }

    /// Adds multiple monitors for a specific network.
    #[tracing::instrument(skip(self, monitors), level = "debug")]
    async fn add_monitors(
        &self,
        network_id: &str,
        monitors: Vec<Monitor>,
    ) -> Result<(), sqlx::Error> {
        tracing::debug!(
            network_id,
            monitor_count = monitors.len(),
            "Adding monitors."
        );

        // Validate that all monitors belong to the correct network
        for monitor in &monitors {
            if monitor.network != network_id {
                tracing::error!(
                    expected_network = network_id,
                    actual_network = monitor.network,
                    monitor_name = monitor.name,
                    "Monitor network mismatch."
                );
                return Err(sqlx::Error::Protocol(format!(
                    "Monitor '{}' has network '{}' but expected '{}'",
                    monitor.name, monitor.network, network_id
                )));
            }
        }

        // Insert monitors in a transaction for atomicity
        let mut tx = self.pool.begin().await?;

        for monitor in monitors {
            sqlx::query(monitor_sql::INSERT_MONITOR)
                .bind(&monitor.name)
                .bind(&monitor.network)
                .bind(&monitor.address)
                .bind(&monitor.filter_script)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;

        tracing::info!(network_id, "Monitors added successfully.");
        Ok(())
    }

    /// Clears all monitors for a specific network.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn clear_monitors(&self, network_id: &str) -> Result<(), sqlx::Error> {
        tracing::debug!(network_id, "Clearing monitors.");

        let result = self
            .execute_query_with_error_handling(
                "clear monitors",
                sqlx::query(monitor_sql::DELETE_MONITORS_BY_NETWORK)
                    .bind(network_id)
                    .execute(&self.pool),
            )
            .await?;

        let deleted_count = result.rows_affected();
        tracing::info!(network_id, deleted_count, "Monitors cleared successfully.");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup_test_db() -> SqliteStateRepository {
        let repo = SqliteStateRepository::new("sqlite::memory:")
            .await
            .expect("Failed to connect to in-memory db");
        repo.run_migrations()
            .await
            .expect("Failed to run migrations");
        repo
    }

    #[tokio::test]
    async fn test_get_and_set_last_processed_block() {
        let repo = setup_test_db().await;
        let network = "mainnet";

        // Initially, should be None
        let block = repo.get_last_processed_block(network).await.unwrap();
        assert!(block.is_none());

        // Set a block number
        repo.set_last_processed_block(network, 12345).await.unwrap();

        // Retrieve it again
        let block = repo.get_last_processed_block(network).await.unwrap();
        assert_eq!(block, Some(12345));

        // Update it
        repo.set_last_processed_block(network, 54321).await.unwrap();

        // Retrieve the updated value
        let block = repo.get_last_processed_block(network).await.unwrap();
        assert_eq!(block, Some(54321));
    }

    #[tokio::test]
    async fn test_cleanup_and_flush_operations() {
        let repo = setup_test_db().await;
        let network = "testnet";

        // Set some data
        repo.set_last_processed_block(network, 100).await.unwrap();

        // Test flush operation
        repo.flush().await.unwrap();

        // Test cleanup operation
        repo.cleanup().await.unwrap();

        // Verify data integrity after cleanup
        let block = repo.get_last_processed_block(network).await.unwrap();
        assert_eq!(block, Some(100));
    }

    #[tokio::test]
    async fn test_emergency_state_saving() {
        let repo = setup_test_db().await;
        let network = "emergency_test";

        // Save emergency state
        repo.save_emergency_state(network, 555, "Test emergency shutdown")
            .await
            .unwrap();

        // Verify the state was saved
        let block = repo.get_last_processed_block(network).await.unwrap();
        assert_eq!(block, Some(555));
    }

    #[tokio::test]
    async fn test_pragma_helper_methods() {
        let repo = setup_test_db().await;

        // Test checkpoint WAL (should not fail even on empty database)
        repo.checkpoint_wal("PASSIVE").await.unwrap();
        repo.checkpoint_wal("TRUNCATE").await.unwrap();

        // Test synchronous mode changes
        repo.set_synchronous_mode("FULL").await.unwrap();
        repo.set_synchronous_mode("NORMAL").await.unwrap();
        repo.set_synchronous_mode("OFF").await.unwrap();
    }

    #[tokio::test]
    async fn test_emergency_state_with_no_prior_state() {
        let repo = setup_test_db().await;
        let network = "fresh_network";

        // Verify no prior state exists
        let initial_state = repo.get_last_processed_block(network).await.unwrap();
        assert!(initial_state.is_none());

        // Save emergency state during first run
        // This simulates a scenario where the application is starting fresh
        // and needs to save an emergency state immediately.
        repo.save_emergency_state(network, 42, "Emergency during first run")
            .await
            .unwrap();

        // Verify the emergency state was saved
        let saved_state = repo.get_last_processed_block(network).await.unwrap();
        assert_eq!(saved_state, Some(42));
    }

    #[tokio::test]
    async fn test_monitor_management_operations() {
        let repo = setup_test_db().await;
        let network_id = "ethereum";

        // Initially, should have no monitors
        let monitors = repo.get_monitors(network_id).await.unwrap();
        assert!(monitors.is_empty());

        // Create test monitors
        let test_monitors = vec![
            Monitor::from_config(
                "USDC Transfer Monitor".to_string(),
                network_id.to_string(),
                "0xa0b86a33e6441b38d4b5e5bfa1bf7a5eb70c5b1e".to_string(),
                r#"log.name == "Transfer" && bigint(log.params.value) > bigint("1000000000")"#
                    .to_string(),
            ),
            Monitor::from_config(
                "DEX Swap Monitor".to_string(),
                network_id.to_string(),
                "0x7a250d5630b4cf539739df2c5dacb4c659f2488d".to_string(),
                r#"log.name == "Swap""#.to_string(),
            ),
        ];

        // Add monitors
        repo.add_monitors(network_id, test_monitors.clone())
            .await
            .unwrap();

        // Retrieve monitors and verify
        let stored_monitors = repo.get_monitors(network_id).await.unwrap();
        assert_eq!(stored_monitors.len(), 2);

        // Check first monitor (order may vary, so find by name)
        let usdc_monitor = stored_monitors
            .iter()
            .find(|m| m.name == "USDC Transfer Monitor")
            .unwrap();
        assert_eq!(usdc_monitor.network, network_id);
        assert_eq!(
            usdc_monitor.address,
            "0xa0b86a33e6441b38d4b5e5bfa1bf7a5eb70c5b1e"
        );
        assert!(usdc_monitor.filter_script.contains("Transfer"));
        assert!(usdc_monitor.id > 0); // Should have been assigned an ID

        // Check second monitor
        let dex_monitor = stored_monitors
            .iter()
            .find(|m| m.name == "DEX Swap Monitor")
            .unwrap();
        assert_eq!(dex_monitor.network, network_id);
        assert_eq!(
            dex_monitor.address,
            "0x7a250d5630b4cf539739df2c5dacb4c659f2488d"
        );
        assert_eq!(dex_monitor.filter_script, r#"log.name == "Swap""#);
        assert!(dex_monitor.id > 0);

        // Clear monitors
        repo.clear_monitors(network_id).await.unwrap();

        // Verify monitors are cleared
        let monitors_after_clear = repo.get_monitors(network_id).await.unwrap();
        assert!(monitors_after_clear.is_empty());
    }

    #[tokio::test]
    async fn test_monitor_network_isolation() {
        let repo = setup_test_db().await;
        let network1 = "ethereum";
        let network2 = "polygon";

        // Create monitors for different networks
        let ethereum_monitors = vec![Monitor::from_config(
            "Ethereum Monitor".to_string(),
            network1.to_string(),
            "0x1111111111111111111111111111111111111111".to_string(),
            "true".to_string(),
        )];

        let polygon_monitors = vec![Monitor::from_config(
            "Polygon Monitor".to_string(),
            network2.to_string(),
            "0x2222222222222222222222222222222222222222".to_string(),
            "true".to_string(),
        )];

        // Add monitors to different networks
        repo.add_monitors(network1, ethereum_monitors)
            .await
            .unwrap();
        repo.add_monitors(network2, polygon_monitors).await.unwrap();

        // Verify network isolation
        let eth_monitors = repo.get_monitors(network1).await.unwrap();
        let poly_monitors = repo.get_monitors(network2).await.unwrap();

        assert_eq!(eth_monitors.len(), 1);
        assert_eq!(poly_monitors.len(), 1);
        assert_eq!(eth_monitors[0].name, "Ethereum Monitor");
        assert_eq!(poly_monitors[0].name, "Polygon Monitor");

        // Clear one network shouldn't affect the other
        repo.clear_monitors(network1).await.unwrap();

        let eth_monitors_after_clear = repo.get_monitors(network1).await.unwrap();
        let poly_monitors_after_clear = repo.get_monitors(network2).await.unwrap();

        assert!(eth_monitors_after_clear.is_empty());
        assert_eq!(poly_monitors_after_clear.len(), 1);
    }

    #[tokio::test]
    async fn test_monitor_network_validation() {
        let repo = setup_test_db().await;
        let network_id = "ethereum";

        // Create monitor with wrong network
        let wrong_network_monitors = vec![Monitor::from_config(
            "Wrong Network Monitor".to_string(),
            "polygon".to_string(), // Different from network_id
            "0x1111111111111111111111111111111111111111".to_string(),
            "true".to_string(),
        )];

        // Should fail due to network mismatch
        let result = repo.add_monitors(network_id, wrong_network_monitors).await;
        assert!(result.is_err());

        // Verify error message contains network information
        let error = result.unwrap_err();
        match error {
            sqlx::Error::Protocol(msg) => {
                assert!(msg.contains("Wrong Network Monitor"));
                assert!(msg.contains("polygon"));
                assert!(msg.contains("ethereum"));
            }
            _ => panic!("Expected Protocol error with network mismatch message"),
        }

        // Verify no monitors were added
        let monitors = repo.get_monitors(network_id).await.unwrap();
        assert!(monitors.is_empty());
    }

    #[tokio::test]
    async fn test_monitor_empty_operations() {
        let repo = setup_test_db().await;
        let network_id = "testnet";

        // Test adding empty vector
        repo.add_monitors(network_id, vec![]).await.unwrap();
        let monitors = repo.get_monitors(network_id).await.unwrap();
        assert!(monitors.is_empty());

        // Test clearing when no monitors exist
        repo.clear_monitors(network_id).await.unwrap();
        let monitors_after_clear = repo.get_monitors(network_id).await.unwrap();
        assert!(monitors_after_clear.is_empty());
    }

    #[tokio::test]
    async fn test_monitor_transaction_atomicity() {
        let repo = setup_test_db().await;
        let network_id = "ethereum";

        // Create a mix of valid and invalid monitors (invalid due to network mismatch)
        let mixed_monitors = vec![
            Monitor::from_config(
                "Valid Monitor".to_string(),
                network_id.to_string(),
                "0x1111111111111111111111111111111111111111".to_string(),
                "true".to_string(),
            ),
            Monitor::from_config(
                "Invalid Monitor".to_string(),
                "wrong_network".to_string(), // This will cause failure
                "0x2222222222222222222222222222222222222222".to_string(),
                "true".to_string(),
            ),
        ];

        // Should fail due to network validation
        let result = repo.add_monitors(network_id, mixed_monitors).await;
        assert!(result.is_err());

        // Verify no monitors were added (transaction rolled back)
        let monitors = repo.get_monitors(network_id).await.unwrap();
        assert!(monitors.is_empty());
    }

    #[tokio::test]
    async fn test_monitor_large_script_handling() {
        let repo = setup_test_db().await;
        let network_id = "ethereum";

        // Create monitor with large filter script
        let large_script = "a".repeat(10000); // 10KB script
        let monitor_with_large_script = vec![Monitor::from_config(
            "Large Script Monitor".to_string(),
            network_id.to_string(),
            "0x1111111111111111111111111111111111111111".to_string(),
            large_script.clone(),
        )];

        // Should handle large scripts
        repo.add_monitors(network_id, monitor_with_large_script)
            .await
            .unwrap();

        // Verify the script was stored correctly
        let stored_monitors = repo.get_monitors(network_id).await.unwrap();
        assert_eq!(stored_monitors.len(), 1);
        assert_eq!(stored_monitors[0].filter_script, large_script);
        assert_eq!(stored_monitors[0].filter_script.len(), 10000);
    }
}
