//! Alert management module

use std::{collections::HashMap, sync::Arc, time::Duration};

use dashmap::DashMap;
use thiserror::Error;
use tokio::sync::Mutex;

use crate::{
    models::{
        alert_manager_state::{AggregationState, ThrottleState},
        monitor_match::MonitorMatch,
        notifier::{NotifierConfig, NotifierPolicy},
    },
    notification::{NotificationPayload, NotificationService, error::NotificationError},
    persistence::traits::GenericStateRepository,
};

/// The AlertManager is responsible for processing monitor matches, applying
/// notification policies (throttling, aggregation, etc.) and submitting
/// notifications to Notification service
pub struct AlertManager<T: GenericStateRepository> {
    /// The notification service used to send alerts
    notification_service: Arc<NotificationService>,

    /// The state repository for storing alert states
    state_repository: Arc<T>,

    /// A map of notifier names to their loaded and validated configurations.
    notifiers: Arc<HashMap<String, NotifierConfig>>,

    /// A map of notifier names to their locks to prevent race conditions.
    notifier_locks: DashMap<String, Arc<Mutex<()>>>,
}

/// Errors that can occur within the AlertManager
#[derive(Debug, Error)]
pub enum AlertManagerError {
    /// Error occurred in the notification service
    #[error("Notification error: {0}")]
    NotificationError(#[from] NotificationError),

    // TODO: replace sqlx error with custom error type
    /// Error occurred in the state repository
    #[error("State repository error: {0}")]
    StateRepositoryError(#[from] sqlx::Error),
}

impl<T: GenericStateRepository + Send + Sync + 'static> AlertManager<T> {
    /// Creates a new AlertManager instance
    pub fn new(
        notification_service: Arc<NotificationService>,
        state_repository: Arc<T>,
        notifiers: Arc<HashMap<String, NotifierConfig>>,
    ) -> Self {
        Self { notification_service, state_repository, notifiers, notifier_locks: DashMap::new() }
    }

    /// Processes a monitor match
    pub async fn process_match(
        &self,
        monitor_match: &MonitorMatch,
    ) -> Result<(), AlertManagerError> {
        let notifier_name = &monitor_match.notifier_name;
        let notifier_config = match self.notifiers.get(notifier_name) {
            Some(config) => config,
            None => {
                tracing::warn!(
                    notifier = %monitor_match.notifier_name,
                    "Notifier configuration not found for monitor match."
                );
                return Err(AlertManagerError::NotificationError(NotificationError::ConfigError(
                    format!("Notifier '{}' not found", monitor_match.notifier_name),
                )));
            }
        };

        match &notifier_config.policy {
            Some(policy) => match policy {
                NotifierPolicy::Throttle(throttle_policy) => {
                    self.handle_throttle(monitor_match, throttle_policy).await?;
                }
                NotifierPolicy::Aggregation(_) => {
                    self.handle_aggregation(monitor_match).await?;
                }
            },
            None => {
                // No policy, send immediately
                tracing::debug!("No policy for notifier {}, sending immediately.", notifier_name);
                if let Err(e) = self
                    .notification_service
                    .execute(NotificationPayload::Single(monitor_match.clone()))
                    .await
                {
                    tracing::error!(
                        "Failed to execute notification for notifier '{}': {}",
                        notifier_name,
                        e
                    );
                }
            }
        }

        Ok(())
    }

    /// Gets or creates a lock for a specific notifier to prevent race
    /// conditions.
    fn get_notifier_lock(&self, notifier_name: &str) -> Arc<Mutex<()>> {
        self.notifier_locks
            .entry(notifier_name.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Handles throttling policy for a monitor match
    async fn handle_throttle(
        &self,
        monitor_match: &MonitorMatch,
        policy: &crate::models::notifier::ThrottlePolicy,
    ) -> Result<(), AlertManagerError> {
        let notifier_name = &monitor_match.notifier_name;
        let lock = self.get_notifier_lock(notifier_name);
        let _guard = lock.lock().await;
        let throttle_state_key = format!("throttle_state:{}", notifier_name);
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
                tracing::error!("Failed to retrieve throttle state for {}: {}", notifier_name, e);
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
                notifier_name,
                throttle_state.count + 1,
                policy.max_count
            );
            if let Err(e) = self
                .notification_service
                .execute(NotificationPayload::Single(monitor_match.clone()))
                .await
            {
                tracing::error!(
                    "Failed to execute notification for throttled notifier '{}': {}",
                    notifier_name,
                    e
                );
            }
            throttle_state.count += 1;
        } else {
            tracing::debug!(
                "Throttling notification for {}. Limit {}/{}",
                notifier_name,
                throttle_state.count,
                policy.max_count
            );
            // Silently drop the notification
        }

        // Save the updated throttle state
        if let Err(e) =
            self.state_repository.set_json_state(&throttle_state_key, &throttle_state).await
        {
            tracing::error!("Failed to save throttle state for {}: {}", notifier_name, e);
        }
        Ok(())
    }

    /// Handles aggregation policy for a monitor match
    async fn handle_aggregation(
        &self,
        monitor_match: &MonitorMatch,
    ) -> Result<(), AlertManagerError> {
        let aggregation_key = &monitor_match.notifier_name;
        let lock = self.get_notifier_lock(aggregation_key);
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

            // We need to find the notifier and its policy to check the window duration.
            let first_match = &state.matches[0];
            let notifier_config = match self.notifiers.get(&first_match.notifier_name) {
                Some(config) => config,
                None => {
                    tracing::warn!(
                        "Notifier configuration not found for aggregation key '{}'. Clearing \
                         state.",
                        state_key
                    );
                    self.state_repository
                        .set_json_state(&state_key, &AggregationState::default())
                        .await?;
                    continue;
                }
            };

            // This is the crucial part: getting the policy to check against.
            let policy = match &notifier_config.policy {
                Some(NotifierPolicy::Aggregation(agg_policy)) => agg_policy,
                _ => {
                    tracing::warn!(
                        "Aggregation policy not found for notifier '{}'. Clearing state.",
                        notifier_config.name
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
                let payload = NotificationPayload::Aggregated {
                    notifier_name: notifier_config.name.clone(),
                    matches: state.matches,
                    template: policy.template.clone(),
                };

                if let Err(e) = self.notification_service.execute(payload).await {
                    tracing::error!(
                        "Failed to send aggregated notification for key '{}': {}",
                        state_key,
                        e
                    );
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

    /// Flushes any pending aggregated notifications.
    pub async fn flush(&self) -> Result<(), AlertManagerError> {
        self.check_and_dispatch_expired_windows(true).await
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
            monitor_match::{LogDetails, LogMatchData, MatchData},
            notifier::{AggregationPolicy, DiscordConfig, NotifierTypeConfig, ThrottlePolicy},
        },
        persistence::traits::MockGenericStateRepository,
    };

    fn create_monitor_match(notifier_name: String) -> MonitorMatch {
        MonitorMatch {
            monitor_id: 0,
            monitor_name: "Test Monitor".to_string(),
            notifier_name,
            block_number: 123,
            transaction_hash: TxHash::default(),
            match_data: MatchData::Log(LogMatchData {
                log_details: LogDetails {
                    contract_address: Address::default(),
                    log_index: 0,
                    name: "Test Log".to_string(),
                    params: json!({
                        "param1": "value1",
                        "param2": 42,
                    }),
                },
                tx_details: json!({}), // Default empty transaction details for test
            }),
        }
    }

    fn create_alert_manager(
        notifiers: HashMap<String, NotifierConfig>,
        state_repo: MockGenericStateRepository,
    ) -> AlertManager<MockGenericStateRepository> {
        let state_repo = Arc::new(state_repo);
        let notifiers_arc = Arc::new(notifiers);
        let notification_service = Arc::new(NotificationService::new(
            notifiers_arc.clone(),
            Arc::new(HttpClientPool::default()),
        ));
        AlertManager::new(notification_service, state_repo, notifiers_arc)
    }

    #[tokio::test]
    async fn test_process_match_notifier_config_missing() {
        // Arrange
        let notifiers = HashMap::new(); // Empty notifiers map
        let state_repo = MockGenericStateRepository::new();
        let alert_manager = create_alert_manager(notifiers, state_repo);
        let monitor_match = create_monitor_match("NonExistentNotifier".to_string());

        // Act
        let result = alert_manager.process_match(&monitor_match).await;

        // Assert
        assert!(matches!(
            result,
            Err(AlertManagerError::NotificationError(NotificationError::ConfigError(_)))
        ));
    }

    #[tokio::test]
    async fn test_process_match_notifier_no_policy_send_immediately() {
        let notifier_name = "Test Notifier".to_string();
        let notifier_config = NotifierConfig {
            name: notifier_name.clone(),
            config: NotifierTypeConfig::Discord(DiscordConfig {
                discord_url: "http://example.com".to_string(),
                message: NotificationMessage {
                    title: "Test".to_string(),
                    body: "This is a test".to_string(),
                },
                retry_policy: Default::default(),
            }),
            policy: None,
        };
        let mut notifiers = HashMap::new();
        notifiers.insert(notifier_name.to_string(), notifier_config);
        let state_repo = MockGenericStateRepository::new();
        let alert_manager = create_alert_manager(notifiers, state_repo);
        let monitor_match = create_monitor_match(notifier_name);

        // Act
        let result = alert_manager.process_match(&monitor_match).await;

        // Assert
        assert!(result.is_ok());
        // Note: Actual notification is tested in integration tests
    }

    #[tokio::test]
    async fn test_process_match_notifier_throttle_new_state() {
        let notifier_name = "Throttle Notifier".to_string();
        let throttle_policy =
            ThrottlePolicy { max_count: 5, time_window_secs: Duration::from_secs(60) };

        let notifier_config = NotifierConfig {
            name: notifier_name.clone(),
            config: NotifierTypeConfig::Discord(DiscordConfig {
                discord_url: "http://example.com".to_string(),
                message: NotificationMessage {
                    title: "Test".to_string(),
                    body: "This is a test".to_string(),
                },
                retry_policy: Default::default(),
            }),
            policy: Some(NotifierPolicy::Throttle(throttle_policy)),
        };
        let mut notifiers = HashMap::new();
        notifiers.insert(notifier_name.to_string(), notifier_config);

        let mut state_repo = MockGenericStateRepository::new();

        // Should attempt to get existing state and find none
        state_repo
            .expect_get_json_state::<ThrottleState>()
            .with(eq(format!("throttle_state:{}", notifier_name.clone())))
            .times(1)
            .returning(|_| Ok(None)); // Simulate no existing state

        // Should attempt to save new throttle state with count = 1
        let notifier_name_for_withf = notifier_name.clone();
        state_repo
            .expect_set_json_state::<ThrottleState>()
            .withf(move |key, state| {
                key == &format!("throttle_state:{}", notifier_name_for_withf) && state.count == 1
            })
            .times(1)
            .returning(|_, _| Ok(())); // Simulate successful state save

        let alert_manager = create_alert_manager(notifiers, state_repo);
        let monitor_match = create_monitor_match(notifier_name);

        let result = alert_manager.process_match(&monitor_match).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_process_match_notifier_throttle_existing_state() {
        let notifier_name = "Throttle Notifier".to_string();
        let throttle_policy =
            ThrottlePolicy { max_count: 5, time_window_secs: Duration::from_secs(60) };

        let notifier_config = NotifierConfig {
            name: notifier_name.clone(),
            config: NotifierTypeConfig::Discord(DiscordConfig {
                discord_url: "http://example.com".to_string(),
                message: NotificationMessage {
                    title: "Test".to_string(),
                    body: "This is a test".to_string(),
                },
                retry_policy: Default::default(),
            }),
            policy: Some(NotifierPolicy::Throttle(throttle_policy)),
        };
        let mut notifiers = HashMap::new();
        notifiers.insert(notifier_name.to_string(), notifier_config);

        let mut state_repo = MockGenericStateRepository::new();

        // Get state for existing throttle
        state_repo
            .expect_get_json_state::<ThrottleState>()
            .with(eq(format!("throttle_state:{}", notifier_name.clone())))
            .times(1)
            .returning(|_| {
                Ok(Some(ThrottleState { count: 1, window_start_time: chrono::Utc::now() }))
            }); // Simulate existing state

        // Should attempt to save new throttle state with count = 2
        let notifier_name_for_withf = notifier_name.clone();
        state_repo
            .expect_set_json_state::<ThrottleState>()
            .withf(move |key, state| {
                key == &format!("throttle_state:{}", notifier_name_for_withf) && state.count == 2
            })
            .times(1)
            .returning(|_, _| Ok(())); // Simulate successful state save

        let alert_manager = create_alert_manager(notifiers, state_repo);
        let monitor_match = create_monitor_match(notifier_name);

        let result = alert_manager.process_match(&monitor_match).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_process_match_notifier_throttle_failed_to_retrieve_state() {
        let notifier_name = "Throttle Notifier".to_string();
        let throttle_policy =
            ThrottlePolicy { max_count: 5, time_window_secs: Duration::from_secs(60) };

        let notifier_config = NotifierConfig {
            name: notifier_name.clone(),
            config: NotifierTypeConfig::Discord(DiscordConfig {
                discord_url: "http://example.com".to_string(),
                message: NotificationMessage {
                    title: "Test".to_string(),
                    body: "This is a test".to_string(),
                },
                retry_policy: Default::default(),
            }),
            policy: Some(NotifierPolicy::Throttle(throttle_policy)),
        };
        let mut notifiers = HashMap::new();
        notifiers.insert(notifier_name.to_string(), notifier_config);

        let mut state_repo = MockGenericStateRepository::new();

        // Fails to retrieve state
        state_repo
            .expect_get_json_state::<ThrottleState>()
            .with(eq(format!("throttle_state:{}", notifier_name.clone())))
            .times(1)
            .returning(|_| Err(sqlx::Error::RowNotFound)); // Simulate retrieval error

        // Should attempt to save new throttle state with count = 1
        let notifier_name_for_withf = notifier_name.clone();
        state_repo
            .expect_set_json_state::<ThrottleState>()
            .withf(move |key, state| {
                key == &format!("throttle_state:{}", notifier_name_for_withf) && state.count == 1
            })
            .times(1)
            .returning(|_, _| Ok(())); // Simulate successful state save

        let alert_manager = create_alert_manager(notifiers, state_repo);
        let monitor_match = create_monitor_match(notifier_name);

        let result = alert_manager.process_match(&monitor_match).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_handle_aggregation_new_window() {
        let notifier_name = "Aggregation Notifier".to_string();
        let aggregation_policy = AggregationPolicy {
            window_secs: Duration::from_secs(1),
            template: NotificationMessage {
                title: "Test".to_string(),
                body: "This is a test".to_string(),
            },
        };

        let notifier_config = NotifierConfig {
            name: notifier_name.clone(),
            config: NotifierTypeConfig::Discord(DiscordConfig {
                discord_url: "http://example.com".to_string(),
                message: NotificationMessage {
                    title: "Test".to_string(),
                    body: "This is a test".to_string(),
                },
                retry_policy: Default::default(),
            }),
            policy: Some(NotifierPolicy::Aggregation(aggregation_policy)),
        };
        let mut notifiers = HashMap::new();
        notifiers.insert(notifier_name.to_string(), notifier_config);

        let mut state_repo = MockGenericStateRepository::new();
        let monitor_match = create_monitor_match(notifier_name.clone());
        let aggregation_key = &monitor_match.notifier_name;
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

        let alert_manager = create_alert_manager(notifiers, state_repo);

        let result = alert_manager.process_match(&monitor_match).await;
        assert!(result.is_ok());

        // Note: The spawned task's behavior is tested by integration tests
    }

    #[tokio::test]
    async fn test_handle_aggregation_existing_window() {
        let notifier_name = "Aggregation Notifier".to_string();
        let aggregation_policy = AggregationPolicy {
            window_secs: Duration::from_secs(60),
            template: NotificationMessage {
                title: "Test".to_string(),
                body: "This is a test".to_string(),
            },
        };

        let notifier_config = NotifierConfig {
            name: notifier_name.clone(),
            config: NotifierTypeConfig::Discord(DiscordConfig {
                discord_url: "http://example.com".to_string(),
                message: NotificationMessage {
                    title: "Test".to_string(),
                    body: "This is a test".to_string(),
                },
                retry_policy: Default::default(),
            }),
            policy: Some(NotifierPolicy::Aggregation(aggregation_policy)),
        };
        let mut notifiers = HashMap::new();
        notifiers.insert(notifier_name.to_string(), notifier_config);

        let mut state_repo = MockGenericStateRepository::new();
        let monitor_match = create_monitor_match(notifier_name.clone());
        let aggregation_key = &monitor_match.notifier_name;
        let state_key = format!("aggregation_state:{}", aggregation_key);

        // Expect a call to get the current state, returning an existing state.
        let existing_match = create_monitor_match(notifier_name.clone());
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

        let alert_manager = create_alert_manager(notifiers, state_repo);

        let result = alert_manager.process_match(&monitor_match).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_check_and_dispatch_expired_windows() {
        let notifier_name = "Aggregation Notifier".to_string();
        let aggregation_policy = AggregationPolicy {
            window_secs: Duration::from_secs(1),
            template: NotificationMessage {
                title: "Aggregated Alert: {{ monitor_name }}".to_string(),
                body: "{{ matches | length }} events detected.".to_string(),
            },
        };

        let notifier_config = NotifierConfig {
            name: notifier_name.clone(),
            config: NotifierTypeConfig::Discord(DiscordConfig {
                discord_url: "http://example.com".to_string(),
                message: NotificationMessage {
                    title: "Test".to_string(),
                    body: "This is a test".to_string(),
                },
                retry_policy: Default::default(),
            }),
            policy: Some(NotifierPolicy::Aggregation(aggregation_policy.clone())),
        };
        let mut notifiers = HashMap::new();
        notifiers.insert(notifier_name.to_string(), notifier_config.clone());

        let mut state_repo = MockGenericStateRepository::new();
        let monitor_match1 = create_monitor_match(notifier_name.clone());
        let monitor_match2 = create_monitor_match(notifier_name.clone());
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

        let alert_manager = create_alert_manager(notifiers, state_repo);

        // Act
        let result = alert_manager.check_and_dispatch_expired_windows(false).await;

        // Assert
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_notifier_lock_is_shared_and_distinct() {
        // Arrange
        let state_repo = MockGenericStateRepository::new();
        let alert_manager = create_alert_manager(HashMap::new(), state_repo);
        let notifier1_name = "notifier1";
        let notifier2_name = "notifier2";

        // Act
        let lock1_instance1 = alert_manager.get_notifier_lock(notifier1_name);
        let lock1_instance2 = alert_manager.get_notifier_lock(notifier1_name);
        let lock2_instance1 = alert_manager.get_notifier_lock(notifier2_name);

        // Assert
        // The same notifier name should return the same lock instance.
        assert!(Arc::ptr_eq(&lock1_instance1, &lock1_instance2));

        // Different notifier names should return different lock instances.
        assert!(!Arc::ptr_eq(&lock1_instance1, &lock2_instance1));
    }
}
