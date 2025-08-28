//! Alert management module

use std::sync::Arc;

use thiserror::Error;

use crate::{
    models::{monitor_match::MonitorMatch, notifier::NotifierPolicy},
    notification::{NotificationService, error::NotificationError},
    persistence::traits::StateRepository,
};

/// The AlertManager is responsible for processing monitor matches, applying
/// notification policies (throttling, aggregation, etc.) and submitting
/// notifications to Notification service
pub struct AlertManager {
    /// The notification service used to send alerts
    notification_service: Arc<NotificationService>,

    /// The state repository for storing alert states
    state_repository: Arc<dyn StateRepository>,
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

impl AlertManager {
    /// Creates a new AlertManager instance
    pub fn new(
        notification_service: Arc<NotificationService>,
        state_repository: Arc<dyn StateRepository>,
    ) -> Self {
        Self { notification_service, state_repository }
    }

    /// Processes a monitor match
    pub async fn process_match(
        &self,
        monitor_match: &MonitorMatch,
    ) -> Result<(), AlertManagerError> {
        let notifier = self.notification_service.notifiers.get(&monitor_match.notifier_name);

        // if notifier not found - log and return error
        let Some(notifier) = notifier else {
            tracing::warn!(notifier = %monitor_match.notifier_name, "Notifier not found");
            return Err(AlertManagerError::NotificationError(NotificationError::ConfigError(
                format!("Notifier '{}' not found", monitor_match.notifier_name),
            )));
        };

        let policy = notifier.policy.as_ref();

        match policy {
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
                // if no policy - send notification
                self.notification_service.execute(monitor_match).await?
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
}
