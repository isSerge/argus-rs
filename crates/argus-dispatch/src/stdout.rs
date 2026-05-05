use std::sync::Arc;

use argus_core::models::action::StdoutConfig;

use crate::{
    ActionPayload, error::ActionDispatcherError, template::TemplateService, traits::Action,
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
    use std::sync::Arc;

    use alloy::primitives::{TxHash, address};
    use argus_core::models::{
        action::StdoutConfig, monitor_match::LogDetails, notification::NotificationMessage,
    };
    use serde_json::json;

    use super::*;
    use crate::{ActionPayload, template::TemplateService};

    fn make_payload(action_name: &str) -> ActionPayload {
        let log_details = LogDetails {
            address: address!("1234567890abcdef1234567890abcdef12345678"),
            log_index: 0,
            name: "Transfer".to_string(),
            params: json!({"from": "0xabc", "value": 100}),
        };
        let monitor_match = argus_core::models::monitor_match::MonitorMatch::builder(
            1,
            "test-monitor".to_string(),
            action_name.to_string(),
            1,
            TxHash::default(),
        )
        .log_match(log_details, json!({}))
        .decoded_call(None)
        .build();
        ActionPayload::Single(monitor_match)
    }

    #[tokio::test]
    async fn execute_with_message_renders_template() {
        let config = StdoutConfig {
            message: Some(NotificationMessage {
                title: "Alert: {{ monitor_name }}".into(),
                body: "Matched on block {{ block_number }}".into(),
            }),
        };
        let action = StdoutAction::new(config, Arc::new(TemplateService::new()));
        let payload = make_payload("test-action");

        let result = action.execute(payload).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn execute_without_message_prints_raw_context() {
        let config = StdoutConfig { message: None };
        let action = StdoutAction::new(config, Arc::new(TemplateService::new()));
        let payload = make_payload("test-action");

        let result = action.execute(payload).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn execute_with_message_returns_error_on_bad_template() {
        let config = StdoutConfig {
            message: Some(NotificationMessage { title: "{{ unclosed".into(), body: "body".into() }),
        };
        let action = StdoutAction::new(config, Arc::new(TemplateService::new()));
        let payload = make_payload("test-action");

        let result = action.execute(payload).await;

        assert!(matches!(result, Err(ActionDispatcherError::TemplateError(_))));
    }
}
