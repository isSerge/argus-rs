//! Alert management module

use std::{collections::HashMap, sync::Arc, time::Duration};

use dashmap::DashMap;
use thiserror::Error;
use tokio::sync::Mutex;

use crate::{
    actions::{ActionDispatcher, ActionPayload, error::ActionDispatcherError},
    models::{
        action::{ActionConfig, ActionPolicy},
        alert_manager_state::{AggregationState, ThrottleState},
        monitor_match::MonitorMatch,
    },
    persistence::{error::PersistenceError, traits::KeyValueStore},
};

/// The AlertManager is responsible for processing monitor matches, applying
/// notification policies (throttling, aggregation, etc.) and submitting
/// notifications to ActionDispatcher
pub struct AlertManager<T: KeyValueStore> {
    /// The action dispatcher used to dispatch actions (webhook notifications,
    /// publishers, etc.)
    action_dispatcher: Arc<ActionDispatcher>,

    /// The state repository for storing alert states
    state_repository: Arc<T>,

    /// A map of action names to their loaded and validated configurations.
    actions: Arc<HashMap<String, ActionConfig>>,

    /// A map of action names to their locks to prevent race conditions.
    action_locks: DashMap<String, Arc<Mutex<()>>>,

    /// Track dispatched notifications for dry-run reporting
    dispatched_notifications: DashMap<String, usize>,
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

impl<T: KeyValueStore> AlertManager<T> {
    /// Creates a new AlertManager instance
    pub fn new(
        action_dispatcher: Arc<ActionDispatcher>,
        state_repository: Arc<T>,
        actions: Arc<HashMap<String, ActionConfig>>,
    ) -> Self {
        Self {
            action_dispatcher,
            state_repository,
            actions,
            action_locks: DashMap::new(),
            dispatched_notifications: DashMap::new(),
        }
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
                // No policy, send immediately
                tracing::debug!("No policy for action {}, sending immediately.", action_name);
                if let Err(e) = self
                    .action_dispatcher
                    .execute(ActionPayload::Single(monitor_match.clone()))
                    .await
                {
                    tracing::error!(
                        "Failed to execute notification for action '{}': {}",
                        action_name,
                        e
                    );
                } else {
                    // Increment dispatch counter on successful notification
                    *self.dispatched_notifications.entry(action_name.clone()).or_insert(0) += 1;
                }
            }
        }

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
        let throttle_state_key = format!("throttle_state:{}", action_name);
        let current_time = chrono::Utc::now();

        // Retrieve existing throttle state or initialize a new one
        let mut throttle_state = match self
            .state_repository
            .get_json_state::<ThrottleState>(&throttle_state_key)
            .await
        {
            Ok(Some(state)) => state,
            Ok(None) => {
                // No state found, initialize new state
                ThrottleState { count: 0, window_start_time: current_time }
            }
            Err(e) => {
                // If there's an error retrieving state, log it and proceed with a new state.
                // This prevents a single state retrieval error from halting throttling.
                tracing::error!("Failed to retrieve throttle state for {}: {}", action_name, e);
                ThrottleState { count: 0, window_start_time: current_time }
            }
        };

        // Check if the throttling window has expired
        if current_time > throttle_state.window_start_time + policy.time_window_secs {
            // Reset the window
            throttle_state.count = 0;
            throttle_state.window_start_time = current_time;
        }

        // Check if the notification should be sent
        if throttle_state.count < policy.max_count {
            tracing::info!(
                "Sending throttled notification for {}. Count: {}/{}",
                action_name,
                throttle_state.count + 1,
                policy.max_count
            );
            if let Err(e) =
                self.action_dispatcher.execute(ActionPayload::Single(monitor_match.clone())).await
            {
                tracing::error!(
                    "Failed to execute notification for throttled action '{}': {}",
                    action_name,
                    e
                );
            } else {
                // Increment dispatch counter on successful notification
                *self
                    .dispatched_notifications
                    .entry(monitor_match.action_name.clone())
                    .or_insert(0) += 1;
            }
            throttle_state.count += 1;
        } else {
            tracing::debug!(
                "Throttling notification for {}. Limit {}/{}",
                action_name,
                throttle_state.count,
                policy.max_count
            );
            // Silently drop the notification
        }

        // Save the updated throttle state
        if let Err(e) =
            self.state_repository.set_json_state(&throttle_state_key, &throttle_state).await
        {
            tracing::error!("Failed to save throttle state for {}: {}", action_name, e);
        }
        Ok(())
    }

    /// Handles aggregation policy for a monitor match
    async fn handle_aggregation(
        &self,
        monitor_match: &MonitorMatch,
    ) -> Result<(), AlertManagerError> {
        let aggregation_key = &monitor_match.action_name;
        let lock = self.get_action_lock(aggregation_key);
        let _guard = lock.lock().await;

        let state_key = format!("aggregation_state:{}", aggregation_key);

        let mut state = self
            .state_repository
            .get_json_state::<AggregationState>(&state_key)
            .await
            .map_err(AlertManagerError::from)?
            .unwrap_or_default();

        // If this is the first match for a new window, set the start time.
        if state.matches.is_empty() {
            state.window_start_time = chrono::Utc::now();
        }

        state.matches.push(monitor_match.clone());

        self.state_repository.set_json_state(&state_key, &state).await?;

        Ok(())
    }

    /// Scans the state repository for expired aggregation windows and
    /// dispatches them.
    async fn check_and_dispatch_expired_windows(
        &self,
        force: bool,
    ) -> Result<(), AlertManagerError> {
        const AGGREGATION_PREFIX: &str = "aggregation_state:";
        let pending_states = self
            .state_repository
            .get_all_json_states_by_prefix::<AggregationState>(AGGREGATION_PREFIX)
            .await?;

        for (state_key, state) in pending_states {
            if state.matches.is_empty() {
                continue;
            }

            // We need to find the action and its policy to check the window duration.
            let first_match = &state.matches[0];
            let action_config = match self.actions.get(&first_match.action_name) {
                Some(config) => config,
                None => {
                    tracing::warn!(
                        "Action configuration not found for aggregation key '{}'. Clearing state.",
                        state_key
                    );
                    self.state_repository
                        .set_json_state(&state_key, &AggregationState::default())
                        .await?;
                    continue;
                }
            };

            // This is the crucial part: getting the policy to check against.
            let policy = match &action_config.policy {
                Some(ActionPolicy::Aggregation(agg_policy)) => agg_policy,
                _ => {
                    tracing::warn!(
                        "Aggregation policy not found for action '{}'. Clearing state.",
                        action_config.name
                    );
                    self.state_repository
                        .set_json_state(&state_key, &AggregationState::default())
                        .await?;
                    continue;
                }
            };

            // Now we use the policy's window_secs to check for expiration.
            if force || chrono::Utc::now() > state.window_start_time + policy.window_secs {
                tracing::info!(
                    "Aggregation window for key '{}' expired. Dispatching summary.",
                    state_key
                );

                // And we use the policy's template to build the payload.
                let payload = ActionPayload::Aggregated {
                    action_name: action_config.name.clone(),
                    matches: state.matches,
                    template: policy.template.clone(),
                };

                if let Err(e) = self.action_dispatcher.execute(payload).await {
                    tracing::error!(
                        "Failed to send aggregated notification for key '{}': {}",
                        state_key,
                        e
                    );
                } else {
                    // Increment dispatch counter on successful notification
                    *self
                        .dispatched_notifications
                        .entry(action_config.name.clone())
                        .or_insert(0) += 1;
                }

                // And finally, clear the state.
                if let Err(e) = self
                    .state_repository
                    .set_json_state(&state_key, &AggregationState::default())
                    .await
                {
                    tracing::error!(
                        "Failed to clear aggregation state for key '{}': {}",
                        state_key,
                        e
                    );
                }
            }
        }
        Ok(())
    }

    /// Runs a background task to dispatch expired aggregation windows.
    /// This should be spawned as a long-running task by the Supervisor.
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

    /// Gets the count of dispatched notifications by action name.
    pub fn get_dispatched_notifications(&self) -> &DashMap<String, usize> {
        &self.dispatched_notifications
    }

    /// Flushes any pending aggregated notifications.
    pub async fn flush(&self) -> Result<(), AlertManagerError> {
        self.check_and_dispatch_expired_windows(true).await
    }

    /// Shuts down the alert manager, flushing any pending notifications and
    /// shutting down the action dispatcher.
    pub async fn shutdown(&self) {
        tracing::info!("Shutting down alert manager...");
        if let Err(e) = self.flush().await {
            tracing::error!("Failed to flush pending notifications: {}", e);
        }
        self.action_dispatcher.shutdown().await;
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
        http_client::HttpClientPool,
        models::{
            NotificationMessage,
            action::{AggregationPolicy, ThrottlePolicy},
            monitor_match::LogDetails,
        },
        persistence::traits::MockKeyValueStore,
        test_helpers::ActionBuilder,
    };

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

    async fn create_alert_manager(
        actions: HashMap<String, ActionConfig>,
        state_repo: MockKeyValueStore,
    ) -> AlertManager<MockKeyValueStore> {
        let state_repo = Arc::new(state_repo);
        let actions_arc = Arc::new(actions);
        let action_dispatcher = Arc::new(
            ActionDispatcher::new(actions_arc.clone(), Arc::new(HttpClientPool::default()))
                .await
                .unwrap(),
        );
        AlertManager::new(action_dispatcher, state_repo, actions_arc)
    }

    #[tokio::test]
    async fn test_process_match_action_config_missing() {
        // Arrange
        let actions = HashMap::new(); // Empty actions map
        let state_repo = MockKeyValueStore::new();
        let alert_manager = create_alert_manager(actions, state_repo).await;
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
        let state_repo = MockKeyValueStore::new();
        let alert_manager = create_alert_manager(actions, state_repo).await;
        let monitor_match = create_monitor_match(action_name);

        // Act
        let result = alert_manager.process_match(&monitor_match).await;

        // Assert
        assert!(result.is_ok());
        // Note: Actual notification is tested in integration tests
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

        let mut state_repo = MockKeyValueStore::new();

        // Should attempt to get existing state and find none
        state_repo
            .expect_get_json_state::<ThrottleState>()
            .with(eq(format!("throttle_state:{}", action_name.clone())))
            .times(1)
            .returning(|_| Ok(None)); // Simulate no existing state

        // Should attempt to save new throttle state with count = 1
        let action_name_for_withf = action_name.clone();
        state_repo
            .expect_set_json_state::<ThrottleState>()
            .withf(move |key, state| {
                key == &format!("throttle_state:{}", action_name_for_withf) && state.count == 1
            })
            .times(1)
            .returning(|_, _| Ok(())); // Simulate successful state save

        let alert_manager = create_alert_manager(actions, state_repo).await;
        let monitor_match = create_monitor_match(action_name);

        let result = alert_manager.process_match(&monitor_match).await;

        assert!(result.is_ok());
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

        let mut state_repo = MockKeyValueStore::new();

        // Get state for existing throttle
        state_repo
            .expect_get_json_state::<ThrottleState>()
            .with(eq(format!("throttle_state:{}", action_name.clone())))
            .times(1)
            .returning(|_| {
                Ok(Some(ThrottleState { count: 1, window_start_time: chrono::Utc::now() }))
            }); // Simulate existing state

        // Should attempt to save new throttle state with count = 2
        let action_name_for_withf = action_name.clone();
        state_repo
            .expect_set_json_state::<ThrottleState>()
            .withf(move |key, state| {
                key == &format!("throttle_state:{}", action_name_for_withf) && state.count == 2
            })
            .times(1)
            .returning(|_, _| Ok(())); // Simulate successful state save

        let alert_manager = create_alert_manager(actions, state_repo).await;
        let monitor_match = create_monitor_match(action_name);

        let result = alert_manager.process_match(&monitor_match).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_process_match_action_throttle_failed_to_retrieve_state() {
        let action_name = "Throttle Action".to_string();
        let throttle_policy =
            ThrottlePolicy { max_count: 5, time_window_secs: Duration::from_secs(60) };

        let action_config = ActionBuilder::new(&action_name)
            .discord_config("http://example.com")
            .policy(ActionPolicy::Throttle(throttle_policy))
            .build();
        let mut actions = HashMap::new();
        actions.insert(action_name.to_string(), action_config);

        let mut state_repo = MockKeyValueStore::new();

        // Fails to retrieve state
        state_repo
            .expect_get_json_state::<ThrottleState>()
            .with(eq(format!("throttle_state:{}", action_name.clone())))
            .times(1)
            .returning(|_| Err(PersistenceError::NotFound)); // Simulate retrieval error

        // Should attempt to save new throttle state with count = 1
        let action_name_for_withf = action_name.clone();
        state_repo
            .expect_set_json_state::<ThrottleState>()
            .withf(move |key, state| {
                key == &format!("throttle_state:{}", action_name_for_withf) && state.count == 1
            })
            .times(1)
            .returning(|_, _| Ok(())); // Simulate successful state save

        let alert_manager = create_alert_manager(actions, state_repo).await;
        let monitor_match = create_monitor_match(action_name);

        let result = alert_manager.process_match(&monitor_match).await;

        assert!(result.is_ok());
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

        let mut state_repo = MockKeyValueStore::new();
        let monitor_match = create_monitor_match(action_name.clone());
        let aggregation_key = &monitor_match.action_name;
        let state_key = format!("aggregation_state:{}", aggregation_key);

        // Expect a call to get the current state, returning None.
        state_repo
            .expect_get_json_state::<AggregationState>()
            .with(eq(state_key.clone()))
            .times(1)
            .returning(|_| Ok(None));

        // Expect a call to save the new state with one match.
        state_repo
            .expect_set_json_state::<AggregationState>()
            .withf(move |key, state| key == state_key && state.matches.len() == 1)
            .times(1)
            .returning(|_, _| Ok(()));

        let alert_manager = create_alert_manager(actions, state_repo).await;

        let result = alert_manager.process_match(&monitor_match).await;
        assert!(result.is_ok());

        // Note: The spawned task's behavior is tested by integration tests
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

        let mut state_repo = MockKeyValueStore::new();
        let monitor_match = create_monitor_match(action_name.clone());
        let aggregation_key = &monitor_match.action_name;
        let state_key = format!("aggregation_state:{}", aggregation_key);

        // Expect a call to get the current state, returning an existing state.
        let existing_match = create_monitor_match(action_name.clone());
        state_repo
            .expect_get_json_state::<AggregationState>()
            .with(eq(state_key.clone()))
            .times(1)
            .returning(move |_| {
                Ok(Some(AggregationState {
                    matches: vec![existing_match.clone()],
                    window_start_time: Utc::now(),
                }))
            });

        // Expect a call to save the updated state with two matches.
        state_repo
            .expect_set_json_state::<AggregationState>()
            .withf(move |key, state| key == state_key && state.matches.len() == 2)
            .times(1)
            .returning(|_, _| Ok(()));

        let alert_manager = create_alert_manager(actions, state_repo).await;

        let result = alert_manager.process_match(&monitor_match).await;
        assert!(result.is_ok());
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

        let mut state_repo = MockKeyValueStore::new();
        let monitor_match1 = create_monitor_match(action_name.clone());
        let monitor_match2 = create_monitor_match(action_name.clone());
        let aggregation_key = &monitor_match1.monitor_name;
        let state_key = format!("aggregation_state:{}", aggregation_key);

        // Simulate an existing aggregation state with two matches and an expired
        // window.
        let expired_state = AggregationState {
            matches: vec![monitor_match1.clone(), monitor_match2.clone()],
            window_start_time: Utc::now() - chrono::Duration::seconds(2),
        };

        let state_key_clone = state_key.clone();
        state_repo
            .expect_get_all_json_states_by_prefix::<AggregationState>()
            .with(eq("aggregation_state:".to_string()))
            .times(1)
            .returning(move |_| Ok(vec![(state_key_clone.clone(), expired_state.clone())]));

        // Expect the state to be cleared after dispatch.
        state_repo
            .expect_set_json_state::<AggregationState>()
            .with(eq(state_key.clone()), eq(AggregationState::default()))
            .times(1)
            .returning(|_, _| Ok(()));

        let alert_manager = create_alert_manager(actions, state_repo).await;

        // Act
        let result = alert_manager.check_and_dispatch_expired_windows(false).await;

        // Assert
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_action_lock_is_shared_and_distinct() {
        // Arrange
        let state_repo = MockKeyValueStore::new();
        let alert_manager = create_alert_manager(HashMap::new(), state_repo).await;
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
        let mut state_repo = MockKeyValueStore::new();

        state_repo
            .expect_get_all_json_states_by_prefix::<AggregationState>()
            .with(eq("aggregation_state:".to_string()))
            .times(1)
            .returning(|_| Ok(vec![])); // No pending states

        let mut actions = HashMap::new();

        actions.insert(publisher_action.name.clone(), publisher_action);

        let alert_manager = create_alert_manager(actions, state_repo).await;

        alert_manager.shutdown().await;
    }
}
