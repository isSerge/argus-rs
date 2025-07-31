//! This module contains the state management logic for the Argus application.

use async_trait::async_trait;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteRow},
    Row, SqlitePool,
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
}

/// A concrete implementation of the StateRepository using SQLite.
pub struct SqliteStateRepository {
    /// The SQLite connection pool used for database operations.
    pool: SqlitePool,
}

impl SqliteStateRepository {
    /// Creates a new instance of SqliteStateRepository with the provided database URL.
    /// This will create the database file if it does not exist.
    pub async fn new(database_url: &str) -> Result<Self, sqlx::Error> {
        let options = SqliteConnectOptions::from_str(database_url)?
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await?;
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
                match block_number.try_into() {
                    Ok(block_number_u64) => Ok(Some(block_number_u64)),
                    Err(_) => Err(sqlx::Error::ColumnDecode {
                        index: "block_number".to_string(),
                        source: Box::new(std::num::TryFromIntError::default()),
                    }),
                }
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
        .bind({
            if block_number > i64::MAX as u64 {
                return Err(sqlx::Error::ColumnIndexOutOfBounds("Block number exceeds i64::MAX".to_string()));
            }
            block_number as i64
        })
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
