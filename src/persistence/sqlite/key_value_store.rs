//! Implementation of the KeyValueStore trait for SqliteStateRepository

use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};

use crate::persistence::{
    error::PersistenceError, sqlite::SqliteStateRepository, traits::KeyValueStore,
};

#[async_trait]
impl KeyValueStore for SqliteStateRepository {
    /// Retrieves a JSON-serializable state object by its key.
    #[tracing::instrument(skip(self), level = "debug")]
    async fn get_json_state<T: DeserializeOwned + Send + Sync + 'static>(
        &self,
        key: &str,
    ) -> Result<Option<T>, PersistenceError> {
        tracing::debug!(key, "Attempting to retrieve JSON state.");

        let result = self
            .execute_query_with_error_handling(
                "get JSON state",
                sqlx::query!("SELECT value FROM application_state WHERE key = ?", key)
                    .fetch_optional(&self.pool),
            )
            .await?;

        match result {
            Some(record) => {
                let value_str: String = record.value;
                serde_json::from_str(&value_str)
                    .map(Some)
                    .map_err(|e| PersistenceError::SerializationError(e.to_string()))
            }
            None => Ok(None),
        }
    }

    /// Sets or updates a JSON-serializable state object by its key.
    #[tracing::instrument(skip(self, value), level = "debug")]
    async fn set_json_state<T: Serialize + Send + Sync + 'static>(
        &self,
        key: &str,
        value: &T,
    ) -> Result<(), PersistenceError> {
        tracing::debug!(key, "Attempting to set JSON state.");

        let value_str = serde_json::to_string(value)
            .map_err(|e| PersistenceError::SerializationError(e.to_string()))?;

        self.execute_query_with_error_handling(
            "set JSON state",
            sqlx::query!(
                "INSERT OR REPLACE INTO application_state (key, value) VALUES (?, ?)",
                key,
                value_str
            )
            .execute(&self.pool),
        )
        .await?;

        Ok(())
    }

    async fn get_all_json_states_by_prefix<T: DeserializeOwned + Send + Sync + 'static>(
        &self,
        prefix: &str,
    ) -> Result<Vec<(String, T)>, PersistenceError> {
        tracing::debug!(prefix = prefix, "Attempting to retrieve all JSON states by prefix.");

        let like_prefix = format!("{}%", prefix);
        let rows = self
            .execute_query_with_error_handling(
                "get all JSON states by prefix",
                sqlx::query!(
                    "SELECT key, value FROM application_state WHERE key LIKE ?",
                    like_prefix
                )
                .fetch_all(&self.pool),
            )
            .await?;

        let mut states = Vec::new();
        for row in rows {
            let key: String = row.key;
            let value_str: String = row.value;
            match serde_json::from_str(&value_str) {
                Ok(value) => states.push((key, value)),
                Err(e) => {
                    tracing::error!(key, "Failed to decode JSON state: {}", e);
                }
            }
        }

        Ok(states)
    }
}
