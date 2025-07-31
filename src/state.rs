//! This module contains the state management logic for the Argus application.

use async_trait::async_trait;
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteRow},
};
use std::str::FromStr;
use tracing;

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
}
