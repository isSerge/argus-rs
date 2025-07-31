//! This module contains the state management logic for the Argus application.

use async_trait::async_trait;
use sqlx::SqlitePool;

/// Represents the state management interface for the Argus application.
#[async_trait]
pub trait StateRepository {
    /// Retrieves the last processed block number for a given network.
    async fn get_last_processed_block(&self, network_id: &str) -> Result<Option<u64>, sqlx::Error>;
    /// Sets the last processed block number for a given network.
    async fn set_last_processed_block(&self, network_id: &str, block_number: u64) -> Result<(), sqlx::Error>;
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
}

#[async_trait]
impl StateRepository for SqliteStateRepository {
    async fn get_last_processed_block(&self, network_id: &str) -> Result<Option<u64>, sqlx::Error> {
        Ok(None)
    }

    async fn set_last_processed_block(&self, network_id: &str, block_number: u64) -> Result<(), sqlx::Error> {
        Ok(())
    }
}
