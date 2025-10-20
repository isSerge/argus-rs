use std::sync::Arc;

use crate::{
    actions::{
        ActionPayload, error::ActionDispatcherError, template::TemplateService, traits::Action,
    },
    models::action::StdoutConfig,
};

/// An action that prints a message to standard output.
pub struct StdoutAction {
    config: StdoutConfig,
    template_service: Arc<TemplateService>,
}

impl StdoutAction {
    /// Creates a new `StdoutAction` with the given configuration and template
    /// service.
    pub fn new(config: StdoutConfig, template_service: Arc<TemplateService>) -> Self {
        Self { config, template_service }
    }
}

#[async_trait::async_trait]
impl Action for StdoutAction {
    async fn execute(&self, payload: ActionPayload) -> Result<(), ActionDispatcherError> {
        let context = payload.context()?;

        if let Some(message) = &self.config.message {
            let rendered_title = self.template_service.render(&message.title, context.clone())?;
            let rendered_body = self.template_service.render(&message.body, context.clone())?;
            println!(
                "=== Stdout Action: {} ===\n{}\n{}\n",
                payload.action_name(),
                rendered_title,
                rendered_body
            );
        } else {
            println!("=== Stdout Action: {} ===\n {}\n", payload.action_name(), context);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::{TxHash, address};
    use serde_json::json;

    use super::*;
    use crate::{
        models::{
            NotificationMessage,
            action::ActionTypeConfig,
            monitor_match::{LogDetails, MonitorMatch},
        },
        test_helpers::ActionBuilder,
    };

    // TODO: use builder
    fn create_mock_monitor_match(action_name: &str) -> MonitorMatch {
        let log_details = LogDetails {
            address: address!("0x1234567890abcdef1234567890abcdef12345678"),
            log_index: 15,
            name: "TestLog".to_string(),
            params: json!({"param1": "value1", "param2": 42}),
        };
        MonitorMatch::new_log_match(
            1,
            "test monitor".to_string(),
            action_name.to_string(),
            123,
            TxHash::default(),
            log_details,
            json!({}),
            None,
        )
    }

    #[tokio::test]
    async fn test_execute_stdout_with_message() {
        let action_config = ActionBuilder::new("stdout_test")
            .stdout_config(Some(NotificationMessage {
                title: "Test Title".to_string(),
                body: "This is a test body.".to_string(),
            }))
            .build();

        let action_payload = ActionPayload::Single(create_mock_monitor_match(&action_config.name));

        let stdout_config = match action_config.config {
            ActionTypeConfig::Stdout(config) => config,
            _ => panic!("Expected StdoutConfig"),
        };

        let stdout_action = StdoutAction::new(stdout_config, Arc::new(TemplateService::new()));

        let result = stdout_action.execute(action_payload).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_stdout_without_message() {
        let action_config = ActionBuilder::new("stdout_test").stdout_config(None).build();

        let action_payload = ActionPayload::Single(create_mock_monitor_match(&action_config.name));

        let stdout_config = match action_config.config {
            ActionTypeConfig::Stdout(config) => config,
            _ => panic!("Expected StdoutConfig"),
        };

        let stdout_action = StdoutAction::new(stdout_config, Arc::new(TemplateService::new()));

        let result = stdout_action.execute(action_payload).await;

        assert!(result.is_ok());
    }
}
