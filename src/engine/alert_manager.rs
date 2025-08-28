//! Alert management module

use std::{collections::HashMap, sync::Arc};

use thiserror::Error;

use crate::{
    models::{
        alert_manager_state::ThrottleState,
        monitor_match::MonitorMatch,
        notifier::{NotifierConfig, NotifierPolicy},
    },
    notification::{NotificationService, error::NotificationError},
    persistence::traits::{GenericStateRepository, StateRepository},
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

impl<T: GenericStateRepository> AlertManager<T> {
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
                        // TODO: handle throttle
                    }
                    NotifierPolicy::Aggregation(aggregation_policy) => {
                        // TODO: handle aggregation
                    }
                }
            }
            None => {
                // No policy, send immediately
                tracing::debug!("No policy for notifier {}, sending immediately.", notifier_name);
                if let Err(e) = self.notification_service.execute(monitor_match).await {
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
}

#[cfg(test)]
mod tests {
    use alloy::primitives::TxHash;
    use serde_json::json;

    use super::*;
    use crate::{
        http_client::HttpClientPool,
        models::{
            NotificationMessage,
            monitor_match::{LogDetails, MatchData},
            notifier::{DiscordConfig, NotifierTypeConfig},
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
    ) -> AlertManager<MockGenericStateRepository> {
        let state_repo = Arc::new(MockGenericStateRepository::new());
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
        let alert_manager = create_alert_manager(notifiers);
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

        let alert_manager = create_alert_manager(notifiers);
        let monitor_match = create_monitor_match(notifier_name);

        // Act
        let result = alert_manager.process_match(&monitor_match).await;

        // Assert
        // Just verify that it completes without error, actual notification is tested in
        // integration tests
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_match_notifier_throttle_new_state() {}

    #[test]
    fn test_process_match_notifier_throttle_existing_state() {}

    #[test]
    fn test_process_match_notifier_throttle_failed_to_retrieve_state() {}
}
