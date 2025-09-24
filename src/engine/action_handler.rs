//! This module defines the `ActionHandler` service.

use std::{collections::HashMap, sync::Arc};

use thiserror::Error;

use super::js_client::JsClient;
use crate::{config::ActionConfig, models::monitor_match::MonitorMatch, monitor::MonitorManager};

/// The `ActionHandler` is responsible for running `on_match` actions
/// associated with a monitor.
pub struct ActionHandler {
    /// A map of action names to their loaded and validated configurations.
    actions: Arc<HashMap<String, ActionConfig>>,
    /// The monitor manager for accessing monitor configurations.
    monitor_manager: Arc<MonitorManager>,
    /// The JavaScript executor client for running action scripts.
    executor_client: Arc<dyn JsClient>,
}

/// Errors that can occur during action execution.
#[derive(Debug, Error)]
pub enum ActionHandlerError {
    /// An error occurred during action execution.
    #[error("Action execution error: {0}")]
    Execution(String),

    /// An error occurred during JSON serialization or deserialization.
    #[error("Serialization/Deserialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

impl ActionHandler {
    /// Creates a new `ActionHandler` instance.
    pub fn new(
        actions: Arc<HashMap<String, ActionConfig>>,
        monitor_manager: Arc<MonitorManager>,
        executor_client: Arc<dyn JsClient>,
    ) -> Self {
        Self { actions, monitor_manager, executor_client }
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
                        let script = std::fs::read_to_string(&action.file).map_err(|e| {
                            ActionHandlerError::Execution(format!(
                                "Failed to read action file {}: {}",
                                action.file.display(),
                                e
                            ))
                        })?;

                        match self.executor_client.submit_script(script, &current_match).await {
                            Ok(exec_response) => {
                                let modified_match =
                                    serde_json::from_value::<MonitorMatch>(exec_response.result)?;
                                current_match = modified_match;
                            }
                            Err(e) => {
                                // If script execution fails, log the error and proceed with the
                                // original match.
                                tracing::error!(
                                    "Failed to execute action '{}' for monitor '{}': {}",
                                    action_name,
                                    monitor.name,
                                    e
                                );
                                break;
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

#[cfg(test)]
mod tests {
    use common_models::ExecutionResponse;

    use super::*;
    use crate::{
        engine::js_client,
        test_helpers::{MonitorBuilder, create_monitor_match, create_test_monitor_manager},
    };

    #[test]
    fn test_action_handler_creation() {
        let actions = Arc::new(HashMap::new());
        let monitor_manager = create_test_monitor_manager(vec![]);
        let mock_js_client = Arc::new(js_client::MockJsClient::new());
        let action_handler = ActionHandler::new(actions, monitor_manager, mock_js_client);

        assert!(action_handler.actions.is_empty());
        assert_eq!(Arc::strong_count(&action_handler.monitor_manager), 1);
        assert_eq!(Arc::strong_count(&action_handler.executor_client), 1);
    }

    #[tokio::test]
    async fn test_action_handler_execute_no_actions() {
        let actions = Arc::new(HashMap::new());
        let monitor_manager = create_test_monitor_manager(vec![]);
        let mock_js_client = Arc::new(js_client::MockJsClient::new());
        let action_handler = ActionHandler::new(actions, monitor_manager, mock_js_client);

        let monitor_match = create_monitor_match("TestNotifier".into());

        let result = action_handler.execute(monitor_match.clone()).await;
        assert!(result.is_ok());
        // No actions configured, should return the original match
        assert_eq!(result.unwrap(), monitor_match);
    }

    #[tokio::test]
    async fn test_action_handler_execute_action() {
        use mockall::predicate::eq;

        use crate::config::ActionConfig;

        let mut actions_map = HashMap::new();
        let temp_dir = tempfile::tempdir().unwrap();
        let action_file_path = temp_dir.path().join("test_action.js");
        std::fs::write(&action_file_path, "context.monitor_name = 'modified_monitor_name';")
            .unwrap();

        actions_map.insert(
            "TestAction".into(),
            ActionConfig { name: "TestAction".into(), file: action_file_path.clone() },
        );

        let actions = Arc::new(actions_map);
        let monitors = vec![
            MonitorBuilder::new()
                .name("Test Monitor".into())
                .on_match(vec!["TestAction".into()])
                .build(),
        ];
        let monitor_manager = create_test_monitor_manager(monitors);
        let mut mock_js_client = js_client::MockJsClient::new();

        let original_match = create_monitor_match("TestNotifier".into());
        let modified_match = {
            let mut m = original_match.clone();
            m.monitor_name = "modified_monitor_name".into();
            m
        };
        let modified_match_clone = modified_match.clone();

        mock_js_client
            .expect_submit_script()
            .with(
                eq(std::fs::read_to_string(&action_file_path).unwrap()),
                eq(original_match.clone()),
            )
            .times(1)
            .returning(move |_, _| {
                let response = ExecutionResponse {
                    result: serde_json::to_value(&modified_match_clone).unwrap(),
                    stdout: "".into(),
                    stderr: "".into(),
                };

                Box::pin(async move { Ok(response) })
            });

        let action_handler = ActionHandler::new(actions, monitor_manager, Arc::new(mock_js_client));

        let result = action_handler.execute(original_match.clone()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), modified_match);
    }

    #[tokio::test]
    async fn test_action_handler_execute_action_not_found() {
        let actions = Arc::new(HashMap::new());
        let monitors = vec![
            MonitorBuilder::new()
                .name("Test Monitor".into())
                .on_match(vec!["NonExistentAction".into()])
                .build(),
        ];
        let monitor_manager = create_test_monitor_manager(monitors);
        let mock_js_client = Arc::new(js_client::MockJsClient::new());
        let action_handler = ActionHandler::new(actions, monitor_manager, mock_js_client);

        let monitor_match = create_monitor_match("TestNotifier".into());

        let result = action_handler.execute(monitor_match.clone()).await;
        assert!(result.is_ok());
        // Action not found, should return the original match
        assert_eq!(result.unwrap(), monitor_match);
    }

    #[tokio::test]
    async fn test_action_handler_execute_monitor_not_found() {
        let actions = Arc::new(HashMap::new());
        let monitor_manager = create_test_monitor_manager(vec![]);
        let mock_js_client = Arc::new(js_client::MockJsClient::new());
        let action_handler = ActionHandler::new(actions, monitor_manager, mock_js_client);

        let monitor_match = create_monitor_match("TestNotifier".into());

        let result = action_handler.execute(monitor_match.clone()).await;
        assert!(result.is_ok());
        // Monitor not found, should return the original match
        assert_eq!(result.unwrap(), monitor_match);
    }

    #[tokio::test]
    async fn test_action_handler_execute_action_execution_error() {
        use mockall::predicate::eq;

        use crate::{config::ActionConfig, engine::js_client::JsExecutorClientError};

        let mut actions_map = HashMap::new();
        let temp_dir = tempfile::tempdir().unwrap();
        let action_file_path = temp_dir.path().join("test_action.js");
        std::fs::write(&action_file_path, "invalid script").unwrap();

        actions_map.insert(
            "TestAction".into(),
            ActionConfig { name: "TestAction".into(), file: action_file_path.clone() },
        );

        let actions = Arc::new(actions_map);
        let monitors = vec![
            MonitorBuilder::new()
                .name("Test Monitor".into())
                .on_match(vec!["TestAction".into()])
                .build(),
        ];
        let monitor_manager = create_test_monitor_manager(monitors);
        let mut mock_js_client = js_client::MockJsClient::new();

        let original_match = create_monitor_match("TestNotifier".into());

        mock_js_client
            .expect_submit_script()
            .with(
                eq(std::fs::read_to_string(&action_file_path).unwrap()),
                eq(original_match.clone()),
            )
            .times(1)
            .returning(|_, _| {
                Box::pin(async move {
                    Err(JsExecutorClientError::PortReadError) // Simulate an error
                })
            });

        let action_handler = ActionHandler::new(actions, monitor_manager, Arc::new(mock_js_client));

        let result = action_handler.execute(original_match.clone()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), original_match);
    }
}
