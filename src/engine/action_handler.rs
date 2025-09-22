//! This module defines the `ActionHandler` service.

use std::{collections::HashMap, sync::Arc};

use thiserror::Error;

use crate::{
    config::ActionConfig, engine::js::execute_action, models::monitor_match::MonitorMatch,
    monitor::MonitorManager,
};

/// The `ActionHandler` is responsible for running `on_match` actions
/// associated with a monitor.
pub struct ActionHandler {
    /// A map of action names to their loaded and validated configurations.
    actions: Arc<HashMap<String, ActionConfig>>,
    /// The monitor manager for accessing monitor configurations.
    monitor_manager: Arc<MonitorManager>,
}

/// Errors that can occur during action execution.
#[derive(Debug, Error)]
pub enum ActionHandlerError {
    /// An error occurred during action execution.
    #[error("Action execution error: {0}")]
    ExecutionError(String),
}

impl ActionHandler {
    /// Creates a new `ActionHandler` instance.
    pub fn new(
        actions: Arc<HashMap<String, ActionConfig>>,
        monitor_manager: Arc<MonitorManager>,
    ) -> Self {
        Self { actions, monitor_manager }
    }

    /// Executes any `on_match` actions associated with the monitor that
    /// produced the match. This method may mutate the `monitor_match` in the
    /// future.
    pub async fn execute(
        &self,
        monitor_match: MonitorMatch,
    ) -> Result<MonitorMatch, ActionHandlerError> {
        let monitor = self
            .monitor_manager
            .load()
            .monitors
            .iter()
            .find(|m| m.monitor.id == monitor_match.monitor_id)
            .map(|cm| cm.monitor.clone());

        if let Some(monitor) = monitor {
            if let Some(on_match) = &monitor.on_match {
                // Execute actions sequentially, passing the modified match to the next action
                let mut current_match = monitor_match.clone();
                for action_name in on_match {
                    if let Some(action) = self.actions.get(action_name) {
                        match execute_action(action, &current_match).await {
                            Ok(modified_match) => {
                                current_match = modified_match;
                            }
                            Err(e) => {
                                tracing::error!(
                                    "Failed to execute action '{}' for monitor '{}': {}",
                                    action_name,
                                    monitor.name,
                                    e
                                );
                                return Err(ActionHandlerError::ExecutionError(e.to_string()));
                            }
                        }
                    } else {
                        tracing::warn!(
                            "Action '{}' not found for monitor '{}'",
                            action_name,
                            monitor.name
                        );
                    }
                }
                Ok(current_match)
            } else {
                // No actions to execute, return the original match
                Ok(monitor_match)
            }
        } else {
            tracing::warn!(
                "Monitor with ID {} not found for match, cannot execute actions.",
                monitor_match.monitor_id
            );

            // If monitor not found, return the original match
            Ok(monitor_match)
        }
    }
}
