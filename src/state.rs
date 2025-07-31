//! This module contains the state management logic for the Argus application.

use async_trait::async_trait;
use sqlx::{sqlite::SqliteRow, Row, SqlitePool};

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
    pub async fn new(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePool::connect(database_url).await?;
        Ok(Self { pool })
    }

    /// Runs database migrations.
    pub async fn run_migrations(&self) -> Result<(), sqlx::migrate::MigrateError> {
        sqlx::migrate!("./migrations").run(&self.pool).await
    }
}

#[async_trait]
impl StateRepository for SqliteStateRepository {
    /// Retrieves the last processed block number for a given network.
    async fn get_last_processed_block(&self, network_id: &str) -> Result<Option<u64>, sqlx::Error> {
        let result: Option<SqliteRow> =
            sqlx::query("SELECT block_number FROM processed_blocks WHERE network_id = ?")
                .bind(network_id)
                .fetch_optional(&self.pool)
                .await?;

        match result {
            Some(row) => {
                let block_number: i64 = row.get("block_number");
                Ok(Some(block_number as u64))
            }
            None => Ok(None),
        }
    }

    /// Sets the last processed block number for a given network.
    async fn set_last_processed_block(
        &self,
        network_id: &str,
        block_number: u64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT OR REPLACE INTO processed_blocks (network_id, block_number) VALUES (?, ?)",
        )
        .bind(network_id)
        .bind(block_number as i64)
        .execute(&self.pool)
        .await?;
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
        let block = repo
            .get_last_processed_block(network)
            .await
            .unwrap();
        assert!(block.is_none());

        // Set a block number
        repo.set_last_processed_block(network, 12345)
            .await
            .unwrap();

        // Retrieve it again
        let block = repo
            .get_last_processed_block(network)
            .await
            .unwrap();
        assert_eq!(block, Some(12345));

        // Update it
        repo.set_last_processed_block(network, 54321)
            .await
            .unwrap();

        // Retrieve the updated value
        let block = repo
            .get_last_processed_block(network)
            .await
            .unwrap();
        assert_eq!(block, Some(54321));
    }
}
