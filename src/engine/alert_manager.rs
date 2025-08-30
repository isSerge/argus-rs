//! Alert management module

use std::{collections::HashMap, sync::Arc};

use thiserror::Error;
use tokio::time::{Duration, sleep};

use crate::{
    models::{
        alert_manager_state::{AggregationState, ThrottleState},
        monitor_match::MonitorMatch,
        notifier::{AggregationPolicy, NotifierConfig, NotifierPolicy},
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
        Self { notification_service, state_repository, notifiers }
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
            Some(policy) => {
                match policy {
                    NotifierPolicy::Throttle(throttle_policy) => {
                        self.handle_throttle(monitor_match, throttle_policy).await?;
                    }
                    NotifierPolicy::Aggregation(aggregation_policy) => {
                        self.handle_aggregation(monitor_match, aggregation_policy).await?;
                    }
                }
            }
            None => {
                // No policy, send immediately
                tracing::debug!("No policy for notifier {}, sending immediately.", notifier_name);
                if let Err(e) =
                    self.notification_service.execute(NotificationPayload::Single(monitor_match.clone())).await
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

    /// Handles throttling policy for a monitor match
    async fn handle_throttle(
        &self,
        monitor_match: &MonitorMatch,
        policy: &crate::models::notifier::ThrottlePolicy,
    ) -> Result<(), AlertManagerError> {
        let notifier_name = &monitor_match.notifier_name;
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
                // Error retrieving state, log and initialize new state
                tracing::error!(
                    "Failed to retrieve throttle state for {}: {}",
                    notifier_name,
                    e
                );
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
        if let Err(e) = self
            .state_repository
            .set_json_state(&throttle_state_key, &throttle_state)
            .await
        {
            tracing::error!("Failed to save throttle state for {}: {}", notifier_name, e);
        }
        Ok(())
    }

    /// Handles aggregation policy for a monitor match
    async fn handle_aggregation(
        &self,
        monitor_match: &MonitorMatch,
        policy: &AggregationPolicy,
    ) -> Result<(), AlertManagerError> {
        // Use monitor name as aggregation key
        let aggregation_key = &monitor_match.monitor_name;
        let state_key = format!("aggregation_state:{}", aggregation_key);

        // Retrieve existing aggregation state or initialize a new one
        let mut state = self
            .state_repository
            .get_json_state::<AggregationState>(&state_key)
            .await?
            .unwrap_or_default();

        // Add the new match to the aggregation state
        let is_new_window = state.matches.is_empty();
        state.matches.push(monitor_match.clone());

        // Save the updated aggregation state
        self.state_repository.set_json_state(&state_key, &state).await?;

        // If this is the first match in a new window, set a timer to send the aggregated
        // notification after the window duration
        if is_new_window {
            let notifier_name = monitor_match.notifier_name.clone();
            let notification_service = self.notification_service.clone();
            let state_repository = self.state_repository.clone();
            let window_duration = policy.window_secs;

            tokio::spawn(async move {
                sleep(Duration::from_secs(window_duration.num_seconds() as u64)).await;

                let state =
                    match state_repository.get_json_state::<AggregationState>(&state_key).await {
                        Ok(Some(state)) => state,
                        _ => {
                            tracing::error!(
                                "Failed to retrieve aggregation state for key: {}",
                                state_key
                            );
                            return;
                        }
                    };

                if let Err(e) = notification_service
                    .execute(NotificationPayload::Aggregated(notifier_name.clone(), state.matches))
                    .await
                {
                    tracing::error!("Failed to send aggregated notification: {}", e);
                }

                // Clear the state by setting it back to default.
                if let Err(e) =
                    state_repository.set_json_state(&state_key, &AggregationState::default()).await
                {
                    tracing::error!("Failed to clear aggregation state: {}", e);
                }
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::TxHash;
    use mockall::predicate::eq;
    use serde_json::json;

    use super::*;
    use crate::{
        http_client::HttpClientPool,
        models::{
            NotificationMessage,
            monitor_match::{LogDetails, MatchData},
            notifier::{DiscordConfig, NotifierTypeConfig, ThrottlePolicy},
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
            match_data: MatchData::Log(LogDetails {
                contract_address: alloy::primitives::Address::default(),
                log_index: 0,
                log_name: "Test Log".to_string(),
                log_params: json!({
                    "param1": "value1",
                    "param2": 42,
                }),
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
            Arc::new(HttpClientPool::new()),
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
        // Just verify that it completes without error, actual notification is tested in
        // integration tests
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_process_match_notifier_throttle_new_state() {
        let notifier_name = "Throttle Notifier".to_string();
        let throttle_policy =
            ThrottlePolicy { max_count: 5, time_window_secs: chrono::Duration::seconds(60) };

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
            ThrottlePolicy { max_count: 5, time_window_secs: chrono::Duration::seconds(60) };

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
            ThrottlePolicy { max_count: 5, time_window_secs: chrono::Duration::seconds(60) };

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
}
