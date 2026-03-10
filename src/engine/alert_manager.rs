//! Alert management module

use std::{collections::HashMap, sync::Arc, time::Duration};

use dashmap::DashMap;
use thiserror::Error;
use tokio::sync::Mutex;

use crate::{
    action_dispatcher::{ActionPayload, error::ActionDispatcherError},
    models::{
        action::{ActionConfig, ActionPolicy},
        alert_manager_state::{AggregationState, ThrottleState},
        monitor_match::MonitorMatch,
    },
    persistence::{
        error::PersistenceError,
        traits::{AppRepository, KeyValueStore},
    },
};

/// The AlertManager is responsible for processing monitor matches, applying
/// notification policies (throttling, aggregation, etc.) and enqueuing
/// notifications to the Outbox for delivery by the OutboxProcessor
#[derive(Debug)]
pub struct AlertManager<T: KeyValueStore> {
    /// The state repository for storing alert states
    state_repository: Arc<T>,

    /// A map of action names to their loaded and validated configurations.
    actions: Arc<HashMap<String, ActionConfig>>,

    /// A map of action names to their locks to prevent race conditions.
    action_locks: DashMap<String, Arc<Mutex<()>>>,

    /// Track generated alerts for dry-run reporting
    generated_alerts: DashMap<String, usize>,

    /// In-memory caches for throttle and aggregation states
    throttle_states: DashMap<String, ThrottleState>,

    /// In-memory cache for aggregation states
    aggregation_states: DashMap<String, AggregationState>,
}

/// Errors that can occur within the AlertManager
#[derive(Debug, Error)]
pub enum AlertManagerError {
    /// Error occurred in the notification service
    #[error("Notification error: {0}")]
    ActionDispatcherError(#[from] ActionDispatcherError),

    /// Error occurred in the state repository
    #[error("State repository error: {0}")]
    StateRepositoryError(#[from] PersistenceError),
}

impl<T: KeyValueStore + AppRepository> AlertManager<T> {
    /// Creates a new AlertManager instance
    pub async fn new(
        state_repository: Arc<T>,
        actions: Arc<HashMap<String, ActionConfig>>,
    ) -> Result<Self, AlertManagerError> {
        let throttle_states = DashMap::new();
        let aggregation_states = DashMap::new();

        // 1. Load Throttle states from SQLite
        let saved_throttles = state_repository
            .get_all_json_states_by_prefix::<ThrottleState>("throttle_state:")
            .await?;

        for (key, state) in saved_throttles {
            let action_name = key.replace("throttle_state:", "");
            throttle_states.insert(action_name, state);
        }

        // 2. Load Aggregation states from SQLite
        let saved_aggs = state_repository
            .get_all_json_states_by_prefix::<AggregationState>("aggregation_state:")
            .await?;

        for (key, state) in saved_aggs {
            let action_name = key.replace("aggregation_state:", "");
            aggregation_states.insert(action_name, state);
        }

        Ok(Self {
            state_repository,
            actions,
            action_locks: DashMap::new(),
            generated_alerts: DashMap::new(),
            throttle_states,
            aggregation_states,
        })
    }

    /// Processes a monitor match
    pub async fn process_match(
        &self,
        monitor_match: &MonitorMatch,
    ) -> Result<(), AlertManagerError> {
        let action_name = &monitor_match.action_name;
        let action_config = match self.actions.get(action_name) {
            Some(config) => config,
            None => {
                tracing::warn!(
                    action = %monitor_match.action_name,
                    "Action configuration not found for monitor match."
                );
                return Err(AlertManagerError::ActionDispatcherError(
                    ActionDispatcherError::ConfigError(format!(
                        "Action '{}' not found",
                        monitor_match.action_name
                    )),
                ));
            }
        };

        match &action_config.policy {
            Some(policy) => match policy {
                ActionPolicy::Throttle(throttle_policy) => {
                    self.handle_throttle(monitor_match, throttle_policy).await?;
                }
                ActionPolicy::Aggregation(_) => {
                    self.handle_aggregation(monitor_match).await?;
                }
            },
            None => {
                // No policy, enqueue immediately
                tracing::debug!("No policy for action {}, enqueuing immediately.", action_name);
                if let Err(e) =
                    self.dispatch_payload(ActionPayload::Single(monitor_match.clone())).await
                {
                    tracing::error!(
                        "Failed to enqueue notification for action '{}': {}",
                        action_name,
                        e
                    );
                }
            }
        }

        Ok(())
    }

    /// Internal helper to enqueue payload to the Outbox
    async fn dispatch_payload(&self, payload: ActionPayload) -> Result<(), AlertManagerError> {
        let action_name = payload.action_name();

        // Enqueue the payload to be processed by the OutboxProcessor
        self.state_repository.enqueue_outbox(&action_name, &payload).await?;

        // Update stats for dry-run visibility
        *self.generated_alerts.entry(action_name).or_insert(0) += 1;

        Ok(())
    }

    /// Gets or creates a lock for a specific action to prevent race
    /// conditions.
    fn get_action_lock(&self, action_name: &str) -> Arc<Mutex<()>> {
        self.action_locks
            .entry(action_name.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Handles throttling policy for a monitor match
    async fn handle_throttle(
        &self,
        monitor_match: &MonitorMatch,
        policy: &crate::models::action::ThrottlePolicy,
    ) -> Result<(), AlertManagerError> {
        let action_name = &monitor_match.action_name;
        let lock = self.get_action_lock(action_name);
        let _guard = lock.lock().await;
        let current_time = chrono::Utc::now();

        // Check if we have existing throttle state for this action
        let should_dispatch = {
            let mut throttle_state = self
                .throttle_states
                .entry(action_name.clone())
                .or_insert_with(|| ThrottleState { count: 0, window_start_time: current_time });

            if current_time > throttle_state.window_start_time + policy.time_window_secs {
                throttle_state.count = 0;
                throttle_state.window_start_time = current_time;
            }

            if throttle_state.count < policy.max_count {
                throttle_state.count += 1;
                true
            } else {
                false
            }
        };

        // If we are within limits, dispatch the notification. Otherwise, we simply skip
        // it.
        if should_dispatch {
            if let Err(e) =
                self.dispatch_payload(ActionPayload::Single(monitor_match.clone())).await
            {
                tracing::error!(
                    "Failed to enqueue notification for throttled action '{}': {}",
                    action_name,
                    e
                );
            }
        } else {
            tracing::debug!("Throttling notification for {}. Limit reached.", action_name);
        }

        Ok(())
    }

    /// Handles aggregation policy for a monitor match
    async fn handle_aggregation(
        &self,
        monitor_match: &MonitorMatch,
    ) -> Result<(), AlertManagerError> {
        let action_name = &monitor_match.action_name;
        let lock = self.get_action_lock(action_name);
        let _guard = lock.lock().await;

        let mut aggregation_state = self
            .aggregation_states
            .entry(action_name.clone())
            .or_insert_with(AggregationState::default);

        // If this is the first match for a new window, set the start time.
        if aggregation_state.matches.is_empty() {
            aggregation_state.window_start_time = chrono::Utc::now();
        }

        aggregation_state.matches.push(monitor_match.clone());

        Ok(())
    }

    /// Scans the state repository for expired aggregation windows and
    /// enqueues them to the Outbox.
    async fn check_and_dispatch_expired_windows(
        &self,
        force: bool,
    ) -> Result<(), AlertManagerError> {
        let current_time = chrono::Utc::now();
        let mut payloads_to_dispatch = Vec::new();

        {
            // Scope the DashMap iterator
            for mut ref_mut in self.aggregation_states.iter_mut() {
                let action_name = ref_mut.key().clone();
                let state = ref_mut.value_mut();

                if state.matches.is_empty() {
                    continue;
                }

                let action_config = match self.actions.get(&action_name) {
                    Some(config) => config,
                    None => {
                        state.matches.clear();
                        continue;
                    }
                };

                if let Some(ActionPolicy::Aggregation(policy)) = &action_config.policy {
                    if force || current_time > state.window_start_time + policy.window_secs {
                        let payload = ActionPayload::Aggregated {
                            action_name: action_name.clone(),
                            matches: std::mem::take(&mut state.matches), // Drain matches safely
                            template: policy.template.clone(),
                        };
                        payloads_to_dispatch.push(payload);

                        // We drained the matches, so the state is implicitly
                        // "cleared" for the next
                        // aggregation window.
                    }
                }
            }
        } // End of DashMap scope

        // Dispatch all expired windows
        for payload in payloads_to_dispatch {
            if let Err(e) = self.dispatch_payload(payload).await {
                tracing::error!("Failed to send aggregated notification: {}", e);
            }
        }

        // Periodically sync all memory states to SQLite
        self.sync_states_to_db().await?;

        Ok(())
    }

    /// Explicitly sync memory to DB
    pub async fn sync_states_to_db(&self) -> Result<(), AlertManagerError> {
        for entry in self.throttle_states.iter() {
            let key = format!("throttle_state:{}", entry.key());
            self.state_repository.set_json_state(&key, entry.value()).await?;
        }
        for entry in self.aggregation_states.iter() {
            let key = format!("aggregation_state:{}", entry.key());
            self.state_repository.set_json_state(&key, entry.value()).await?;
        }
        Ok(())
    }

    /// Runs a background task to enqueue expired aggregation windows to the
    /// Outbox. This should be spawned as a long-running task by the
    /// Supervisor.
    pub async fn run_aggregation_dispatcher(&self, check_interval: Duration) {
        let mut interval = tokio::time::interval(check_interval);

        loop {
            interval.tick().await;
            tracing::debug!("Running aggregation dispatcher check...");

            // Log errors from the check, but we never stop the loop.
            if let Err(e) = self.check_and_dispatch_expired_windows(false).await {
                tracing::error!("Error in aggregation dispatcher cycle: {}", e);
            }
        }
    }

    /// Gets the count of generated alerts by action name.
    pub fn get_generated_alerts(&self) -> &DashMap<String, usize> {
        &self.generated_alerts
    }

    /// Flushes any pending aggregated notifications to the Outbox.
    pub async fn flush(&self) -> Result<(), AlertManagerError> {
        self.check_and_dispatch_expired_windows(true).await
    }

    /// Gracefully shuts down the AlertManager, ensuring all pending
    /// notifications are flushed.
    pub async fn shutdown(&self) {
        tracing::info!("Shutting down alert manager...");
        if let Err(e) = self.flush().await {
            tracing::error!("Failed to flush pending notifications: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::{Address, TxHash};
    use chrono::Utc;
    use mockall::predicate::eq;
    use serde_json::json;

    use super::*;
    use crate::{
        models::{
            NotificationMessage,
            action::{AggregationPolicy, ThrottlePolicy},
            monitor_match::LogDetails,
        },
        persistence::traits::{AppRepository, MockAppRepository, MockKeyValueStore},
        test_helpers::ActionBuilder,
    };

    /// Combined mock implementing both KeyValueStore and AppRepository
    #[derive(Debug)]
    struct CombinedMock {
        kv_mock: MockKeyValueStore,
        repo_mock: MockAppRepository,
    }

    #[async_trait::async_trait]
    impl KeyValueStore for CombinedMock {
        async fn get_json_state<T: serde::de::DeserializeOwned + Send + Sync + 'static>(
            &self,
            key: &str,
        ) -> Result<Option<T>, PersistenceError> {
            self.kv_mock.get_json_state(key).await
        }

        async fn set_json_state<T: serde::Serialize + Send + Sync + 'static>(
            &self,
            key: &str,
            value: &T,
        ) -> Result<(), PersistenceError> {
            self.kv_mock.set_json_state(key, value).await
        }

        async fn get_all_json_states_by_prefix<
            T: serde::de::DeserializeOwned + Send + Sync + 'static,
        >(
            &self,
            prefix: &str,
        ) -> Result<Vec<(String, T)>, PersistenceError> {
            self.kv_mock.get_all_json_states_by_prefix(prefix).await
        }
    }

    #[async_trait::async_trait]
    impl AppRepository for CombinedMock {
        async fn get_last_processed_block(
            &self,
            network_id: &str,
        ) -> Result<Option<u64>, PersistenceError> {
            self.repo_mock.get_last_processed_block(network_id).await
        }

        async fn set_last_processed_block(
            &self,
            network_id: &str,
            block_number: u64,
        ) -> Result<(), PersistenceError> {
            self.repo_mock.set_last_processed_block(network_id, block_number).await
        }

        async fn cleanup(&self) -> Result<(), PersistenceError> {
            self.repo_mock.cleanup().await
        }

        async fn flush(&self) -> Result<(), PersistenceError> {
            self.repo_mock.flush().await
        }

        async fn save_emergency_state(
            &self,
            network_id: &str,
            block_number: u64,
            note: &str,
        ) -> Result<(), PersistenceError> {
            self.repo_mock.save_emergency_state(network_id, block_number, note).await
        }

        async fn get_monitors(
            &self,
            network_id: &str,
        ) -> Result<Vec<crate::models::monitor::Monitor>, PersistenceError> {
            self.repo_mock.get_monitors(network_id).await
        }

        async fn get_monitor_by_id(
            &self,
            network_id: &str,
            monitor_id: &str,
        ) -> Result<Option<crate::models::monitor::Monitor>, PersistenceError> {
            self.repo_mock.get_monitor_by_id(network_id, monitor_id).await
        }

        async fn add_monitors(
            &self,
            network_id: &str,
            monitors: Vec<crate::models::monitor::MonitorConfig>,
        ) -> Result<(), PersistenceError> {
            self.repo_mock.add_monitors(network_id, monitors).await
        }

        async fn clear_monitors(&self, network_id: &str) -> Result<(), PersistenceError> {
            self.repo_mock.clear_monitors(network_id).await
        }

        async fn delete_monitor(
            &self,
            network_id: &str,
            monitor_id: &str,
        ) -> Result<(), PersistenceError> {
            self.repo_mock.delete_monitor(network_id, monitor_id).await
        }

        async fn update_monitor(
            &self,
            network_id: &str,
            monitor_id: &str,
            monitor: crate::models::monitor::MonitorConfig,
        ) -> Result<(), PersistenceError> {
            self.repo_mock.update_monitor(network_id, monitor_id, monitor).await
        }

        async fn update_monitor_status(
            &self,
            network_id: &str,
            monitor_id: &str,
            status: crate::models::monitor::MonitorStatus,
        ) -> Result<(), PersistenceError> {
            self.repo_mock.update_monitor_status(network_id, monitor_id, status).await
        }

        async fn create_abi(&self, name: &str, abi: &str) -> Result<(), PersistenceError> {
            self.repo_mock.create_abi(name, abi).await
        }

        async fn get_abi(&self, name: &str) -> Result<Option<String>, PersistenceError> {
            self.repo_mock.get_abi(name).await
        }

        async fn list_abis(&self) -> Result<Vec<String>, PersistenceError> {
            self.repo_mock.list_abis().await
        }

        async fn delete_abi(&self, name: &str) -> Result<(), PersistenceError> {
            self.repo_mock.delete_abi(name).await
        }

        async fn get_all_abis(&self) -> Result<Vec<(String, String)>, PersistenceError> {
            self.repo_mock.get_all_abis().await
        }

        async fn get_actions(
            &self,
            network_id: &str,
        ) -> Result<Vec<ActionConfig>, PersistenceError> {
            self.repo_mock.get_actions(network_id).await
        }

        async fn get_action_by_id(
            &self,
            network_id: &str,
            action_id: i64,
        ) -> Result<Option<ActionConfig>, PersistenceError> {
            self.repo_mock.get_action_by_id(network_id, action_id).await
        }

        async fn get_action_by_name(
            &self,
            network_id: &str,
            name: &str,
        ) -> Result<Option<ActionConfig>, PersistenceError> {
            self.repo_mock.get_action_by_name(network_id, name).await
        }

        async fn create_action(
            &self,
            network_id: &str,
            action: ActionConfig,
        ) -> Result<ActionConfig, PersistenceError> {
            self.repo_mock.create_action(network_id, action).await
        }

        async fn clear_actions(&self, network_id: &str) -> Result<(), PersistenceError> {
            self.repo_mock.clear_actions(network_id).await
        }

        async fn update_action(
            &self,
            network_id: &str,
            action: ActionConfig,
        ) -> Result<ActionConfig, PersistenceError> {
            self.repo_mock.update_action(network_id, action).await
        }

        async fn delete_action(
            &self,
            network_id: &str,
            action_id: i64,
        ) -> Result<(), PersistenceError> {
            self.repo_mock.delete_action(network_id, action_id).await
        }

        async fn get_monitors_by_action_id(
            &self,
            network_id: &str,
            action_id: i64,
        ) -> Result<Vec<crate::models::monitor::MonitorConfig>, PersistenceError> {
            self.repo_mock.get_monitors_by_action_id(network_id, action_id).await
        }

        async fn enqueue_outbox(
            &self,
            action_name: &str,
            payload: &ActionPayload,
        ) -> Result<(), PersistenceError> {
            self.repo_mock.enqueue_outbox(action_name, payload).await
        }

        async fn get_pending_outbox(
            &self,
            limit: i64,
        ) -> Result<Vec<crate::persistence::traits::OutboxItem>, PersistenceError> {
            self.repo_mock.get_pending_outbox(limit).await
        }

        async fn delete_outbox_item(&self, id: i64) -> Result<(), PersistenceError> {
            self.repo_mock.delete_outbox_item(id).await
        }

        async fn increment_outbox_retries(&self, id: i64) -> Result<(), PersistenceError> {
            self.repo_mock.increment_outbox_retries(id).await
        }
    }

    fn create_monitor_match(action_name: String) -> MonitorMatch {
        MonitorMatch::builder(1, "Test Monitor".to_string(), action_name, 123, TxHash::default())
            .log_match(
                LogDetails {
                    address: Address::default(),
                    log_index: 0,
                    name: "Test Log".to_string(),
                    params: json!({
                        "param1": "value1",
                        "param2": 42,
                    }),
                },
                json!({}), // Default empty transaction details for test
            )
            .decoded_call(None)
            .build()
    }

    /// Helper to mock standard DB responses for AlertManager::new (empty
    /// states)
    fn setup_default_kv_mock(kv_mock: &mut MockKeyValueStore) {
        kv_mock
            .expect_get_all_json_states_by_prefix::<ThrottleState>()
            .with(eq("throttle_state:"))
            .returning(|_| Ok(vec![]));

        kv_mock
            .expect_get_all_json_states_by_prefix::<AggregationState>()
            .with(eq("aggregation_state:"))
            .returning(|_| Ok(vec![]));
    }

    async fn create_alert_manager(
        actions: HashMap<String, ActionConfig>,
        kv_mock: MockKeyValueStore,
        repo_mock: MockAppRepository,
    ) -> AlertManager<CombinedMock> {
        let combined_state_mock = Arc::new(CombinedMock { kv_mock, repo_mock });
        let actions_arc = Arc::new(actions);
        AlertManager::new(combined_state_mock, actions_arc).await.unwrap()
    }

    #[tokio::test]
    async fn test_process_match_action_config_missing() {
        // Arrange
        let actions = HashMap::new(); // Empty actions map
        let mut kv_mock = MockKeyValueStore::new();
        setup_default_kv_mock(&mut kv_mock);
        let repo_mock = MockAppRepository::new();
        let alert_manager = create_alert_manager(actions, kv_mock, repo_mock).await;
        let monitor_match = create_monitor_match("NonExistentAction".to_string());

        // Act
        let result = alert_manager.process_match(&monitor_match).await;

        // Assert
        assert!(matches!(
            result,
            Err(AlertManagerError::ActionDispatcherError(ActionDispatcherError::ConfigError(_)))
        ));
    }

    #[tokio::test]
    async fn test_process_match_action_no_policy_send_immediately() {
        let action_name = "Test Action".to_string();
        let action_config =
            ActionBuilder::new(&action_name).discord_config("http://example.com").build();
        let mut actions = HashMap::new();
        actions.insert(action_name.to_string(), action_config);
        let mut kv_mock = MockKeyValueStore::new();
        setup_default_kv_mock(&mut kv_mock);
        let mut repo_mock = MockAppRepository::new();

        // Expect enqueue_outbox to be called
        repo_mock.expect_enqueue_outbox().times(1).returning(|_, _| Ok(()));

        let alert_manager = create_alert_manager(actions, kv_mock, repo_mock).await;
        let monitor_match = create_monitor_match(action_name);

        // Act
        let result = alert_manager.process_match(&monitor_match).await;

        // Assert
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_process_match_action_throttle_new_state() {
        let action_name = "Throttle Action".to_string();
        let throttle_policy =
            ThrottlePolicy { max_count: 5, time_window_secs: Duration::from_secs(60) };

        let action_config = ActionBuilder::new(&action_name)
            .discord_config("http://example.com")
            .policy(ActionPolicy::Throttle(throttle_policy))
            .build();

        let mut actions = HashMap::new();
        actions.insert(action_name.to_string(), action_config);

        let mut kv_mock = MockKeyValueStore::new();
        setup_default_kv_mock(&mut kv_mock);
        let mut repo_mock = MockAppRepository::new();

        // Expect enqueue_outbox to be called
        repo_mock.expect_enqueue_outbox().times(1).returning(|_, _| Ok(()));

        let alert_manager = create_alert_manager(actions, kv_mock, repo_mock).await;
        let monitor_match = create_monitor_match(action_name);

        let result = alert_manager.process_match(&monitor_match).await;

        assert!(result.is_ok());
        // Assert memory state was updated
        assert_eq!(alert_manager.throttle_states.get("Throttle Action").unwrap().count, 1);
    }

    #[tokio::test]
    async fn test_process_match_action_throttle_existing_state() {
        let action_name = "Throttle Action".to_string();
        let throttle_policy =
            ThrottlePolicy { max_count: 5, time_window_secs: Duration::from_secs(60) };

        let action_config = ActionBuilder::new(&action_name)
            .discord_config("http://example.com")
            .policy(ActionPolicy::Throttle(throttle_policy))
            .build();

        let mut actions = HashMap::new();
        actions.insert(action_name.to_string(), action_config);

        let mut kv_mock = MockKeyValueStore::new();
        let action_name_clone = action_name.clone();

        // 1. Return an existing ThrottleState during initialization
        kv_mock
            .expect_get_all_json_states_by_prefix::<ThrottleState>()
            .with(eq("throttle_state:"))
            .times(1)
            .returning(move |_| {
                Ok(vec![(
                    format!("throttle_state:{}", action_name_clone),
                    ThrottleState { count: 1, window_start_time: chrono::Utc::now() },
                )])
            });

        kv_mock
            .expect_get_all_json_states_by_prefix::<AggregationState>()
            .with(eq("aggregation_state:"))
            .returning(|_| Ok(vec![]));

        let mut repo_mock = MockAppRepository::new();
        repo_mock.expect_enqueue_outbox().times(1).returning(|_, _| Ok(()));

        let alert_manager = create_alert_manager(actions, kv_mock, repo_mock).await;
        let monitor_match = create_monitor_match(action_name);

        let result = alert_manager.process_match(&monitor_match).await;

        assert!(result.is_ok());
        // Assert count was correctly incremented from the loaded 1 to 2
        assert_eq!(alert_manager.throttle_states.get("Throttle Action").unwrap().count, 2);
    }

    #[tokio::test]
    async fn test_alert_manager_new_failed_to_retrieve_state() {
        // If the database fails during initialization, AlertManager should propagate
        // the error
        let mut kv_mock = MockKeyValueStore::new();
        let repo_mock = MockAppRepository::new();

        kv_mock
            .expect_get_all_json_states_by_prefix::<ThrottleState>()
            .with(eq("throttle_state:"))
            .times(1)
            .returning(|_| Err(PersistenceError::OperationFailed("DB Offline".to_string())));

        let actions = Arc::new(HashMap::new());
        let combined_mock = Arc::new(CombinedMock { kv_mock, repo_mock });

        let result = AlertManager::new(combined_mock, actions).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AlertManagerError::StateRepositoryError(_)));
    }

    #[tokio::test]
    async fn test_handle_aggregation_new_window() {
        let action_name = "Aggregation Action".to_string();
        let aggregation_policy = AggregationPolicy {
            window_secs: Duration::from_secs(1),
            template: NotificationMessage {
                title: "Test".to_string(),
                body: "This is a test".to_string(),
            },
        };

        let action_config = ActionBuilder::new(&action_name)
            .discord_config("http://example.com")
            .policy(ActionPolicy::Aggregation(aggregation_policy))
            .build();

        let mut actions = HashMap::new();
        actions.insert(action_name.to_string(), action_config);

        let mut kv_mock = MockKeyValueStore::new();
        setup_default_kv_mock(&mut kv_mock);
        let repo_mock = MockAppRepository::new();
        let alert_manager = create_alert_manager(actions, kv_mock, repo_mock).await;
        let monitor_match = create_monitor_match(action_name.clone());

        let result = alert_manager.process_match(&monitor_match).await;
        assert!(result.is_ok());

        // Ensure memory state was populated
        let state = alert_manager.aggregation_states.get(&action_name).unwrap();
        assert_eq!(state.matches.len(), 1);
    }

    #[tokio::test]
    async fn test_handle_aggregation_existing_window() {
        let action_name = "Aggregation Action".to_string();
        let aggregation_policy = AggregationPolicy {
            window_secs: Duration::from_secs(60),
            template: NotificationMessage {
                title: "Test".to_string(),
                body: "This is a test".to_string(),
            },
        };

        let action_config = ActionBuilder::new(&action_name)
            .discord_config("http://example.com")
            .policy(ActionPolicy::Aggregation(aggregation_policy))
            .build();

        let mut actions = HashMap::new();
        actions.insert(action_name.to_string(), action_config);

        let mut kv_mock = MockKeyValueStore::new();
        let action_name_clone = action_name.clone();

        kv_mock
            .expect_get_all_json_states_by_prefix::<ThrottleState>()
            .with(eq("throttle_state:"))
            .returning(|_| Ok(vec![]));

        // Load existing aggregation state
        let existing_match = create_monitor_match(action_name.clone());
        kv_mock
            .expect_get_all_json_states_by_prefix::<AggregationState>()
            .with(eq("aggregation_state:"))
            .times(1)
            .returning(move |_| {
                Ok(vec![(
                    format!("aggregation_state:{}", action_name_clone),
                    AggregationState {
                        matches: vec![existing_match.clone()],
                        window_start_time: Utc::now(),
                    },
                )])
            });

        let repo_mock = MockAppRepository::new();
        let alert_manager = create_alert_manager(actions, kv_mock, repo_mock).await;

        // Add second match
        let monitor_match = create_monitor_match(action_name.clone());
        let result = alert_manager.process_match(&monitor_match).await;
        assert!(result.is_ok());

        // Validate memory state has 2 matches
        let state = alert_manager.aggregation_states.get(&action_name).unwrap();
        assert_eq!(state.matches.len(), 2);
    }

    #[tokio::test]
    async fn test_check_and_dispatch_expired_windows() {
        let action_name = "Aggregation Action".to_string();
        let aggregation_policy = AggregationPolicy {
            window_secs: Duration::from_secs(1),
            template: NotificationMessage {
                title: "Aggregated Alert: {{ monitor_name }}".to_string(),
                body: "{{ matches | length }} events detected.".to_string(),
            },
        };

        let action_config = ActionBuilder::new(&action_name)
            .discord_config("http://example.com")
            .policy(ActionPolicy::Aggregation(aggregation_policy))
            .build();

        let mut actions = HashMap::new();
        actions.insert(action_name.to_string(), action_config.clone());

        let mut kv_mock = MockKeyValueStore::new();
        let action_name_clone = action_name.clone();

        kv_mock
            .expect_get_all_json_states_by_prefix::<ThrottleState>()
            .with(eq("throttle_state:"))
            .returning(|_| Ok(vec![]));

        // Simulate an existing aggregation state with an EXPIRED window
        let monitor_match1 = create_monitor_match(action_name.clone());
        let monitor_match2 = create_monitor_match(action_name.clone());
        let expired_state = AggregationState {
            matches: vec![monitor_match1.clone(), monitor_match2.clone()],
            window_start_time: Utc::now() - chrono::Duration::seconds(5),
        };

        kv_mock
            .expect_get_all_json_states_by_prefix::<AggregationState>()
            .with(eq("aggregation_state:"))
            .times(1)
            .returning(move |_| {
                Ok(vec![(
                    format!("aggregation_state:{}", action_name_clone),
                    expired_state.clone(),
                )])
            });

        // Expect the state to be saved (synced to DB) AFTER dispatch, which should now
        // be empty.
        let state_key = format!("aggregation_state:{}", action_name);
        kv_mock
            .expect_set_json_state::<AggregationState>()
            .withf(move |key, state| key == state_key.as_str() && state.matches.is_empty())
            .times(1)
            .returning(|_, _| Ok(()));

        // Expect enqueue_outbox to be called once for the aggregated payload
        let mut repo_mock = MockAppRepository::new();
        repo_mock.expect_enqueue_outbox().times(1).returning(|_, _| Ok(()));

        let alert_manager = create_alert_manager(actions, kv_mock, repo_mock).await;

        // Act
        let result = alert_manager.check_and_dispatch_expired_windows(false).await;

        // Assert
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_action_lock_is_shared_and_distinct() {
        // Arrange
        let mut kv_mock = MockKeyValueStore::new();
        setup_default_kv_mock(&mut kv_mock);
        let repo_mock = MockAppRepository::new();
        let alert_manager = create_alert_manager(HashMap::new(), kv_mock, repo_mock).await;
        let action1_name = "action1";
        let action2_name = "action2";

        // Act
        let lock1_instance1 = alert_manager.get_action_lock(action1_name);
        let lock1_instance2 = alert_manager.get_action_lock(action1_name);
        let lock2_instance1 = alert_manager.get_action_lock(action2_name);

        // Assert
        // The same action name should return the same lock instance.
        assert!(Arc::ptr_eq(&lock1_instance1, &lock1_instance2));

        // Different action names should return different lock instances.
        assert!(!Arc::ptr_eq(&lock1_instance1, &lock2_instance1));
    }

    #[tokio::test]
    async fn test_shutdown_actions() {
        let publisher_action =
            ActionBuilder::new("Test Action").kafka_config("kafka:9092", "test_topic").build();
        let mut kv_mock = MockKeyValueStore::new();
        setup_default_kv_mock(&mut kv_mock);
        let repo_mock = MockAppRepository::new();

        let mut actions = HashMap::new();

        actions.insert(publisher_action.name.clone(), publisher_action);

        let alert_manager = create_alert_manager(actions, kv_mock, repo_mock).await;

        alert_manager.shutdown().await;
    }
}
