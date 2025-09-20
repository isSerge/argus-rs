//! This module defines the `ActionHandler` service.

use std::{collections::HashMap, sync::Arc};

use crate::{
    config::ActionConfig, engine::js::JavaScriptRunner, models::monitor_match::MonitorMatch,
    monitor::MonitorManager,
};

/// The `ActionHandler` is responsible for running `on_match` actions
/// associated with a monitor.
pub struct ActionHandler {
    /// The JavaScript runner for executing actions.
    js_runner: JavaScriptRunner,
    /// A map of action names to their loaded and validated configurations.
    actions: Arc<HashMap<String, ActionConfig>>,
    /// The monitor manager for accessing monitor configurations.
    monitor_manager: Arc<MonitorManager>,
}

impl ActionHandler {
    /// Creates a new `ActionHandler` instance.
    pub fn new(
        js_runner: JavaScriptRunner,
        actions: Arc<HashMap<String, ActionConfig>>,
        monitor_manager: Arc<MonitorManager>,
    ) -> Self {
        Self { js_runner, actions, monitor_manager }
    }

    /// Executes any `on_match` actions associated with the monitor that
    /// produced the match. This method may mutate the `monitor_match` in the
    /// future.
    pub async fn execute(&self, monitor_match: &mut MonitorMatch) {
        let monitor = self
            .monitor_manager
            .load()
            .monitors
            .iter()
            .find(|m| m.monitor.id == monitor_match.monitor_id)
            .map(|cm| cm.monitor.clone());

        if let Some(monitor) = monitor {
            if let Some(on_match) = &monitor.on_match {
                for action_name in on_match {
                    if let Some(action) = self.actions.get(action_name) {
                        // Note: When data enrichment is implemented, the runner will
                        // need to accept a mutable reference to monitor_match.
                        if let Err(e) = self.js_runner.execute_action(action, monitor_match).await {
                            tracing::error!(
                                "Error executing action '{}' for monitor '{}': {}",
                                action_name,
                                monitor.name,
                                e
                            );
                        }
                    } else {
                        tracing::warn!(
                            "Action '{}' not found for monitor '{}'",
                            action_name,
                            monitor.name
                        );
                    }
                }
            }
        } else {
            tracing::warn!(
                "Monitor with ID {} not found for match, cannot execute actions.",
                monitor_match.monitor_id
            );
        }
    }
}
