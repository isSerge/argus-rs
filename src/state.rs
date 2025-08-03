//! This module contains the state management logic for the Argus application.

use async_trait::async_trait;
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteRow},
};
use std::str::FromStr;

/// Represents the state management interface for the Argus application.
#[async_trait]
pub trait StateRepository {
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
    async fn save_emergency_state(&self, network_id: &str, block_number: u64, note: &str) -> Result<(), sqlx::Error>;
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
}

#[async_trait]
impl StateRepository for SqliteStateRepository {
    /// Retrieves the last processed block number for a given network.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn get_last_processed_block(&self, network_id: &str) -> Result<Option<u64>, sqlx::Error> {
        tracing::debug!(network_id, "Querying for last processed block.");
        let result: Option<SqliteRow> = sqlx::query(
            "SELECT block_number FROM processed_blocks WHERE network_id = ?",
        )
        .bind(network_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, network_id, "Failed to query last processed block.");
            e
        })?;

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
        sqlx::query(
            "INSERT OR REPLACE INTO processed_blocks (network_id, block_number) VALUES (?, ?)",
        )
        .bind(network_id)
        .bind(
            i64::try_from(block_number).map_err(|error| {
                tracing::error!(error = %error, block_number, "Failed to convert block_number to i64 for database insertion.");
                sqlx::Error::ColumnDecode {
                    index: "block_number".to_string(),
                    source: Box::new(error),
                }
            })?,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, network_id, block_number, "Failed to set last processed block.");
            e
        })?;
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
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.pool)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to checkpoint WAL during cleanup.");
                e
            })?;
            
        tracing::debug!("State repository cleanup completed.");
        Ok(())
    }

    /// Ensures all pending writes are flushed to disk.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn flush(&self) -> Result<(), sqlx::Error> {
        tracing::debug!("Flushing pending writes to disk.");
        
        // Execute PRAGMA synchronous to ensure data is written to disk
        sqlx::query("PRAGMA synchronous = FULL")
            .execute(&self.pool)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to set synchronous mode during flush.");
                e
            })?;
            
        // Force a checkpoint to flush WAL to main database
        sqlx::query("PRAGMA wal_checkpoint(PASSIVE)")
            .execute(&self.pool)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to checkpoint WAL during flush.");
                e
            })?;
            
        tracing::debug!("Pending writes flushed successfully.");
        Ok(())
    }

    /// Saves emergency state during shutdown (e.g., partial progress).
    #[tracing::instrument(skip(self), level = "debug")]
    async fn save_emergency_state(&self, network_id: &str, block_number: u64, note: &str) -> Result<(), sqlx::Error> {
        tracing::warn!(
            network_id = %network_id,
            block_number = %block_number,
            note = %note,
            "Saving emergency state during shutdown."
        );
        
        // Save the current state
        self.set_last_processed_block(network_id, block_number).await?;
        
        // Log the emergency save for audit purposes
        tracing::info!(
            network_id = %network_id,
            block_number = %block_number,
            note = %note,
            "Emergency state saved successfully."
        );
        
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
}
