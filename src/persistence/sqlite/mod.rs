//! This module provides a concrete implementation of the StateRepository using
//! SQLite.

use std::str::FromStr;

use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};

pub mod app_repository;
pub mod key_value_store;

use crate::persistence::error::PersistenceError;

/// A concrete implementation of the AppRepository using SQLite.
pub struct SqliteStateRepository {
    /// The SQLite connection pool used for database operations.
    pool: SqlitePool,
}

impl SqliteStateRepository {
    /// Creates a new instance of SqliteStateRepository with the provided
    /// database URL. This will create the database file if it does not
    /// exist.
    #[tracing::instrument(level = "info")]
    pub async fn new(database_url: &str) -> Result<Self, PersistenceError> {
        tracing::debug!(database_url, "Attempting to connect to SQLite database.");
        let options = SqliteConnectOptions::from_str(database_url)
            .map_err(|e| PersistenceError::InvalidInput(e.to_string()))?
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await.map_err(|e| {
            PersistenceError::OperationFailed(format!("Failed to connect to database: {}", e))
        })?;
        tracing::info!(database_url, "Successfully connected to SQLite database.");
        Ok(Self { pool })
    }

    /// Runs database migrations.
    #[tracing::instrument(skip(self), level = "info")]
    pub async fn run_migrations(&self) -> Result<(), PersistenceError> {
        tracing::debug!("Running database migrations.");
        sqlx::migrate!("./migrations").run(&self.pool).await.map_err(|e| {
            tracing::error!(error = %e, "Failed to run database migrations.");
            PersistenceError::MigrationError(e.to_string())
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
    async fn execute_pragma(&self, pragma: &str, operation: &str) -> Result<(), PersistenceError> {
        sqlx::query(pragma)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, pragma = %pragma, operation = %operation, "Failed to execute PRAGMA command.");
                PersistenceError::OperationFailed(e.to_string())
            })?;
        Ok(())
    }

    /// Performs a WAL checkpoint with the specified mode
    async fn checkpoint_wal(&self, mode: &str) -> Result<(), PersistenceError> {
        let allowed_modes = ["PASSIVE", "TRUNCATE", "RESTART", "RESTART_OR_TRUNCATE"];
        if !allowed_modes.contains(&mode) {
            return Err(PersistenceError::InvalidInput(format!(
                "Invalid WAL checkpoint mode: {}",
                mode
            )));
        }
        let pragma = format!("PRAGMA wal_checkpoint({mode})");
        self.execute_pragma(&pragma, &format!("WAL checkpoint {mode}")).await
    }

    /// Sets the synchronous mode
    async fn set_synchronous_mode(&self, mode: &str) -> Result<(), PersistenceError> {
        let allowed_modes = ["OFF", "NORMAL", "FULL"];
        if !allowed_modes.contains(&mode) {
            return Err(PersistenceError::InvalidInput(format!(
                "Invalid synchronous mode: {}",
                mode
            )));
        }
        let pragma = format!("PRAGMA synchronous = {mode}");
        self.execute_pragma(&pragma, &format!("set synchronous mode to {mode}")).await
    }

    /// Helper to execute database queries with consistent error handling
    async fn execute_query_with_error_handling<F, T, E>(
        &self,
        operation: &str,
        query_fn: F,
    ) -> Result<T, PersistenceError>
    where
        F: std::future::Future<Output = Result<T, E>>,
        E: std::error::Error,
    {
        query_fn.await.map_err(|e| {
            tracing::error!(error = %e, operation = %operation, "Database operation failed.");
            PersistenceError::OperationFailed(e.to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::{
        models::{
            monitor::MonitorConfig,
            notification::NotificationMessage,
            notifier::{
                AggregationPolicy, DiscordConfig, NotifierConfig, NotifierPolicy,
                NotifierTypeConfig, SlackConfig, ThrottlePolicy,
            },
        },
        persistence::traits::{AppRepository, KeyValueStore},
    };

    async fn setup_test_db() -> SqliteStateRepository {
        let repo = SqliteStateRepository::new("sqlite::memory:")
            .await
            .expect("Failed to connect to in-memory db");
        repo.run_migrations().await.expect("Failed to run migrations");
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
        repo.save_emergency_state(network, 555, "Test emergency shutdown").await.unwrap();

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
        repo.save_emergency_state(network, 42, "Emergency during first run").await.unwrap();

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
            MonitorConfig::from_config(
                "USDC Transfer Monitor".to_string(),
                network_id.to_string(),
                Some("0xa0b86a33e6441b38d4b5e5bfa1bf7a5eb70c5b1e".to_string()),
                Some("usdc".to_string()),
                r#"log.name == "Transfer" && bigint(log.params.value) > bigint("1000000000")"#
                    .to_string(),
                vec!["test-notifier".to_string()],
            ),
            MonitorConfig::from_config(
                "Simple Transfer Monitor".to_string(),
                network_id.to_string(),
                Some("0x7a250d5630b4cf539739df2c5dacb4c659f2488d".to_string()),
                Some("test".to_string()),
                r#"log.name == "Transfer""#.to_string(),
                vec![],
            ),
            MonitorConfig::from_config(
                "Native ETH Monitor".to_string(),
                network_id.to_string(),
                None, // No address for transaction-level monitor
                None,
                r#"bigint(tx.value) > bigint("1000000000000000000")"#.to_string(),
                vec!["eth-notifier".to_string(), "another-notifier".to_string()],
            ),
        ];

        // Add monitors
        repo.add_monitors(network_id, test_monitors.clone()).await.unwrap();

        // Retrieve monitors and verify
        let stored_monitors = repo.get_monitors(network_id).await.unwrap();
        assert_eq!(stored_monitors.len(), 3);

        // Check USDC monitor
        let usdc_monitor =
            stored_monitors.iter().find(|m| m.name == "USDC Transfer Monitor").unwrap();
        assert_eq!(usdc_monitor.network, network_id);
        assert_eq!(
            usdc_monitor.address,
            Some("0xa0b86a33e6441b38d4b5e5bfa1bf7a5eb70c5b1e".to_string())
        );
        assert!(usdc_monitor.id > 0);
        assert_eq!(usdc_monitor.notifiers, vec!["test-notifier".to_string()]);

        // Check Native ETH monitor
        let eth_monitor = stored_monitors.iter().find(|m| m.name == "Native ETH Monitor").unwrap();
        assert_eq!(eth_monitor.network, network_id);
        assert_eq!(eth_monitor.address, None);
        assert!(eth_monitor.id > 0);
        assert_eq!(
            eth_monitor.notifiers,
            vec!["eth-notifier".to_string(), "another-notifier".to_string()]
        );

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
        let ethereum_monitors = vec![MonitorConfig::from_config(
            "Ethereum Monitor".to_string(),
            network1.to_string(),
            Some("0x1111111111111111111111111111111111111111".to_string()),
            Some("test".to_string()),
            "true".to_string(),
            vec![],
        )];

        let polygon_monitors = vec![MonitorConfig::from_config(
            "Polygon Monitor".to_string(),
            network2.to_string(),
            Some("0x2222222222222222222222222222222222222222".to_string()),
            Some("test".to_string()),
            "true".to_string(),
            vec![],
        )];

        // Add monitors to different networks
        repo.add_monitors(network1, ethereum_monitors).await.unwrap();
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
        let wrong_network_monitors = vec![MonitorConfig::from_config(
            "Wrong Network Monitor".to_string(),
            "polygon".to_string(), // Different from network_id
            Some("0x1111111111111111111111111111111111111111".to_string()),
            Some("test".to_string()),
            "true".to_string(),
            vec![],
        )];

        // Should fail due to network mismatch
        let result = repo.add_monitors(network_id, wrong_network_monitors).await;
        assert!(result.is_err());

        // Verify error message contains network information
        let error = result.unwrap_err();
        match error {
            PersistenceError::InvalidInput(msg) => {
                assert!(msg.contains("Wrong Network Monitor"));
                assert!(msg.contains("polygon"));
                assert!(msg.contains("ethereum"));
            }
            _ => panic!("Expected InvalidInput error with network mismatch message"),
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
            MonitorConfig::from_config(
                "Valid Monitor".to_string(),
                network_id.to_string(),
                Some("0x1111111111111111111111111111111111111111".to_string()),
                Some("test".to_string()),
                "true".to_string(),
                vec![],
            ),
            MonitorConfig::from_config(
                "Invalid Monitor".to_string(),
                "wrong_network".to_string(), // This will cause failure
                Some("0x2222222222222222222222222222222222222222".to_string()),
                Some("test".to_string()),
                "true".to_string(),
                vec![],
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
        let monitor_with_large_script = vec![MonitorConfig::from_config(
            "Large Script Monitor".to_string(),
            network_id.to_string(),
            Some("0x1111111111111111111111111111111111111111".to_string()),
            Some("test".to_string()),
            large_script.clone(),
            vec![],
        )];

        // Should handle large scripts
        repo.add_monitors(network_id, monitor_with_large_script).await.unwrap();

        // Verify the script was stored correctly
        let stored_monitors = repo.get_monitors(network_id).await.unwrap();
        assert_eq!(stored_monitors.len(), 1);
        assert_eq!(stored_monitors[0].filter_script, large_script);
        assert_eq!(stored_monitors[0].filter_script.len(), 10000);
    }

    #[tokio::test]
    async fn test_notifier_management_operations() {
        let repo = setup_test_db().await;
        let network_id = "ethereum";

        // Initially, should have no notifiers
        let notifiers = repo.get_notifiers(network_id).await.unwrap();
        assert!(notifiers.is_empty());

        // Create test notifiers
        let test_notifiers = vec![NotifierConfig {
            name: "Test Slack".to_string(),
            config: NotifierTypeConfig::Slack(SlackConfig {
                slack_url: "https://hooks.slack.com/services/123".to_string(),
                message: NotificationMessage {
                    title: "Test".to_string(),
                    body: "Body".to_string(),
                    ..Default::default()
                },
                retry_policy: Default::default(),
            }),
            policy: None,
        }];

        // Add notifiers
        repo.add_notifiers(network_id, test_notifiers.clone()).await.unwrap();

        // Retrieve notifiers and verify
        let stored_notifiers = repo.get_notifiers(network_id).await.unwrap();
        assert_eq!(stored_notifiers.len(), 1);
        assert_eq!(stored_notifiers[0].name, "Test Slack");

        // Clear notifiers
        repo.clear_notifiers(network_id).await.unwrap();

        // Verify notifiers are cleared
        let notifiers_after_clear = repo.get_notifiers(network_id).await.unwrap();
        assert!(notifiers_after_clear.is_empty());
    }

    #[tokio::test]
    async fn test_notifier_network_isolation() {
        let repo = setup_test_db().await;
        let network1 = "ethereum";
        let network2 = "polygon";

        // Create notifiers for different networks
        let ethereum_notifiers = vec![NotifierConfig {
            name: "Ethereum Slack".to_string(),
            config: NotifierTypeConfig::Slack(SlackConfig {
                slack_url: "https://hooks.slack.com/services/eth".to_string(),
                message: Default::default(),
                retry_policy: Default::default(),
            }),
            policy: None,
        }];
        let polygon_notifiers = vec![NotifierConfig {
            name: "Polygon Discord".to_string(),
            config: NotifierTypeConfig::Discord(DiscordConfig {
                discord_url: "https://discord.com/api/webhooks/poly".to_string(),
                message: Default::default(),
                retry_policy: Default::default(),
            }),
            policy: None,
        }];

        // Add notifiers to different networks
        repo.add_notifiers(network1, ethereum_notifiers).await.unwrap();
        repo.add_notifiers(network2, polygon_notifiers).await.unwrap();

        // Verify network isolation
        let eth_notifiers = repo.get_notifiers(network1).await.unwrap();
        let poly_notifiers = repo.get_notifiers(network2).await.unwrap();

        assert_eq!(eth_notifiers.len(), 1);
        assert_eq!(poly_notifiers.len(), 1);
        assert_eq!(eth_notifiers[0].name, "Ethereum Slack");
        assert_eq!(poly_notifiers[0].name, "Polygon Discord");

        // Clear one network, should not affect the other
        repo.clear_notifiers(network1).await.unwrap();
        let eth_notifiers_after_clear = repo.get_notifiers(network1).await.unwrap();
        let poly_notifiers_after_clear = repo.get_notifiers(network2).await.unwrap();

        assert!(eth_notifiers_after_clear.is_empty());
        assert_eq!(poly_notifiers_after_clear.len(), 1);
    }

    #[tokio::test]
    async fn test_notifier_transaction_atomicity() {
        let repo = setup_test_db().await;
        let network_id = "ethereum";

        // Create a batch of notifiers where one has a name that is too long, causing a
        // DB constraint error. This test is a bit contrived as we can't easily
        // create an invalid JSON, but we can simulate a constraint violation.
        // Here, we'll rely on the UNIQUE constraint.
        let notifiers1 = vec![
            NotifierConfig {
                name: "Unique Notifier".to_string(),
                config: NotifierTypeConfig::Slack(SlackConfig::default()),
                policy: None,
            },
            NotifierConfig {
                name: "Another Unique Notifier".to_string(),
                config: NotifierTypeConfig::Slack(SlackConfig::default()),
                policy: None,
            },
        ];
        let notifiers2 = vec![
            NotifierConfig {
                name: "Third Notifier".to_string(),
                config: NotifierTypeConfig::Slack(SlackConfig::default()),
                policy: None,
            },
            NotifierConfig {
                name: "Unique Notifier".to_string(), // Duplicate name, will cause failure
                config: NotifierTypeConfig::Slack(SlackConfig::default()),
                policy: None,
            },
        ];

        // This should succeed
        repo.add_notifiers(network_id, notifiers1).await.unwrap();
        assert_eq!(repo.get_notifiers(network_id).await.unwrap().len(), 2);

        // This should fail due to the duplicate name violating the UNIQUE constraint
        let result = repo.add_notifiers(network_id, notifiers2).await;
        assert!(result.is_err());

        // Verify no new notifiers were added (transaction rolled back)
        // The count should still be 2 from the first successful insert.
        assert_eq!(repo.get_notifiers(network_id).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_notifier_policy_persistence() {
        let repo = setup_test_db().await;
        let network_id = "testnet_policy";

        let aggregation_policy = NotifierPolicy::Aggregation(AggregationPolicy {
            window_secs: Duration::from_secs(60),
            template: NotificationMessage {
                title: "Aggregated Alert".to_string(),
                body: "Multiple events occurred.".to_string(),
            },
        });

        let test_notifier = NotifierConfig {
            name: "Policy Notifier".to_string(),
            config: NotifierTypeConfig::Slack(SlackConfig {
                slack_url: "https://hooks.slack.com/services/policy".to_string(),
                message: NotificationMessage {
                    title: "Policy Test".to_string(),
                    body: "Body".to_string(),
                    ..Default::default()
                },
                retry_policy: Default::default(),
            }),
            policy: Some(aggregation_policy.clone()),
        };

        // Add notifier with policy
        repo.add_notifiers(network_id, vec![test_notifier.clone()]).await.unwrap();

        // Retrieve and verify
        let stored_notifiers = repo.get_notifiers(network_id).await.unwrap();
        assert_eq!(stored_notifiers.len(), 1);
        let retrieved_notifier = &stored_notifiers[0];

        assert_eq!(retrieved_notifier.name, test_notifier.name);
        assert!(retrieved_notifier.policy.is_some());
        assert_eq!(retrieved_notifier.policy.as_ref().unwrap(), &aggregation_policy);

        // Test with a throttle policy as well
        let throttle_policy = NotifierPolicy::Throttle(ThrottlePolicy {
            max_count: 5,
            time_window_secs: Duration::from_secs(10),
        });

        let throttled_notifier = NotifierConfig {
            name: "Throttled Notifier".to_string(),
            config: NotifierTypeConfig::Discord(DiscordConfig {
                discord_url: "https://discord.com/api/webhooks/throttle".to_string(),
                message: NotificationMessage {
                    title: "Throttle Test".to_string(),
                    body: "Body".to_string(),
                    ..Default::default()
                },
                retry_policy: Default::default(),
            }),
            policy: Some(throttle_policy.clone()),
        };

        repo.add_notifiers(network_id, vec![throttled_notifier.clone()]).await.unwrap();

        let stored_notifiers_after_throttle = repo.get_notifiers(network_id).await.unwrap();
        assert_eq!(stored_notifiers_after_throttle.len(), 2);

        let retrieved_throttled_notifier = stored_notifiers_after_throttle
            .into_iter()
            .find(|n| n.name == "Throttled Notifier")
            .unwrap();

        assert!(retrieved_throttled_notifier.policy.is_some());
        assert_eq!(retrieved_throttled_notifier.policy.unwrap(), throttle_policy);
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct TestJsonState {
        id: u32,
        message: String,
        is_active: bool,
    }

    #[tokio::test]
    async fn test_json_state_persistence() {
        let repo = setup_test_db().await;
        let key = "test_generic_state";

        // Initially, should be None
        let retrieved: Option<TestJsonState> = repo.get_json_state(key).await.unwrap();
        assert!(retrieved.is_none());

        // Set a state
        let original_state =
            TestJsonState { id: 1, message: "Hello, World!".to_string(), is_active: true };
        repo.set_json_state(key, &original_state).await.unwrap();

        // Retrieve it again
        let retrieved_state: Option<TestJsonState> = repo.get_json_state(key).await.unwrap();
        assert_eq!(retrieved_state, Some(original_state.clone()));

        // Update it
        let updated_state =
            TestJsonState { id: 1, message: "Updated message.".to_string(), is_active: false };
        repo.set_json_state(key, &updated_state).await.unwrap();

        // Retrieve the updated value
        let retrieved_updated_state: Option<TestJsonState> =
            repo.get_json_state(key).await.unwrap();
        assert_eq!(retrieved_updated_state, Some(updated_state.clone()));

        // Test a different key, should be None
        let non_existent: Option<TestJsonState> =
            repo.get_json_state("non_existent_key").await.unwrap();
        assert!(non_existent.is_none());
    }
}
