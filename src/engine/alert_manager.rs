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
        models::monitor_match::{LogDetails, MatchData},
    };

    #[tokio::test]
    async fn test_process_match_notifier_config_missing() {
        // Arrange
        let mock_state_repo =
            Arc::new(crate::persistence::traits::MockGenericStateRepository::new());
        let notifiers = Arc::new(HashMap::new()); // Empty notifiers map
        let notification_service =
            Arc::new(NotificationService::new(notifiers.clone(), Arc::new(HttpClientPool::new())));
        let alert_manager = AlertManager::new(notification_service, mock_state_repo, notifiers);

        let monitor_match = MonitorMatch {
            monitor_id: 0,
            monitor_name: "Test Monitor".to_string(),
            notifier_name: "Missing Notifier".to_string(),
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
        };

        // Act
        let result = alert_manager.process_match(&monitor_match).await;

        // Assert
        assert!(matches!(
            result,
            Err(AlertManagerError::NotificationError(NotificationError::ConfigError(_)))
        ));
    }

    #[test]
    fn test_process_match_notifier_no_policy() {}

    #[test]
    fn test_process_match_notifier_throttle_new_state() {}

    #[test]
    fn test_process_match_notifier_throttle_existing_state() {}

    #[test]
    fn test_process_match_notifier_throttle_failed_to_retrieve_state() {}
}
