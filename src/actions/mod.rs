//! # Action Dispatcher
//!
//! This module is responsible for sending notifications through various
//! channels based on action configurations. It acts as the central hub for
//! dispatching alerts when a monitor finds a match.
//!
//! ## Core Components
//!
//! - **`ActionDispatcher`**: The main struct that holds the loaded action
//!   configurations and a shared `HttpClientPool`. It is responsible for
//!   executing notifications.
//! - **`Action` Trait**: A generic interface for all notification channels,
//!   allowing for a unified dispatch mechanism.
//!
//! ## Workflow
//!
//! 1. The `ActionDispatcher` is initialized with a collection of validated
//!    `ActionConfig`s, which are loaded at application startup.
//! 2. For each `ActionConfig`, a corresponding `Action` implementation (e.g.,
//!    `WebhookClientWrapper`, `StdoutAction`) is created and stored.
//! 3. When a monitor match occurs, the `execute` method is called with the name
//!    of the action to be executed.
//! 4. The manager looks up the corresponding `Action` trait object and calls
//!    its `notify` method with the appropriate payload.

use std::{collections::HashMap, sync::Arc};

use crate::{
    http_client::HttpClientPool,
    models::{
        NotificationMessage,
        action::{ActionConfig, ActionTypeConfig, StdoutConfig},
        monitor_match::MonitorMatch,
    },
};

pub mod error;
mod payload;
mod stdout;
pub mod template;
mod traits;
mod webhook;

use error::ActionDispatcherError;
pub use payload::ActionPayload;
use tokio::sync::mpsc;
use webhook::WebhookComponents;

use self::{template::TemplateService, webhook::WebhookClient};

impl ActionTypeConfig {
    /// Transforms the specific action configuration into a generic set of
    /// webhook components.
    fn as_webhook_components(&self) -> Result<WebhookComponents, ActionDispatcherError> {
        Ok(match self {
            ActionTypeConfig::Webhook(c) => c.into(),
            ActionTypeConfig::Discord(c) => c.into(),
            ActionTypeConfig::Telegram(c) => c.into(),
            ActionTypeConfig::Slack(c) => c.into(),
            ActionTypeConfig::Stdout(_) =>
                return Err(ActionDispatcherError::ConfigError(
                    "Stdout action does not support webhook components".to_string(),
                )),
        })
    }
}

/// A service responsible for dispatching actions based on pre-loaded
/// action configurations (webhook notifiers, publishers, etc.)
pub struct ActionDispatcher {
    /// A thread-safe pool for creating and reusing HTTP clients with different
    /// retry policies.
    client_pool: Arc<HttpClientPool>,
    /// A map of action names to their loaded and validated configurations.
    actions: Arc<HashMap<String, ActionConfig>>,
    /// The service for rendering notification templates.
    template_service: TemplateService,
}

impl ActionDispatcher {
    /// Creates a new `ActionDispatcher` instance.
    ///
    /// # Arguments
    ///
    /// * `actions` - A vector of `ActionConfig` loaded and validated at
    ///   application startup.
    /// * `client_pool` - A shared pool of HTTP clients.
    pub fn new(
        actions: Arc<HashMap<String, ActionConfig>>,
        client_pool: Arc<HttpClientPool>,
    ) -> Self {
        ActionDispatcher { client_pool, actions, template_service: TemplateService::new() }
    }

    /// Executes a notification for a given action.
    ///
    /// This method looks up the action by name, constructs the necessary
    /// components, and dispatches the notification.
    ///
    /// # Arguments
    ///
    /// * `payload` - The notification payload, which can be either a single
    ///   match or an aggregated summary.
    ///
    /// # Returns
    ///
    /// * `Result<(), ActionDispatcherError>` - Returns `Ok(())` on success, or
    ///   a `ActionDispatcherError` if the action is not found, the HTTP client
    ///   fails, or the notification fails to send.
    pub async fn execute(&self, payload: ActionPayload) -> Result<(), ActionDispatcherError> {
        let (action_name, context, custom_template) = match payload {
            ActionPayload::Single(monitor_match) => {
                let context = serde_json::to_value(&monitor_match).map_err(|e| {
                    ActionDispatcherError::InternalError(format!(
                        "Failed to serialize monitor match: {e}"
                    ))
                })?;
                (monitor_match.action_name, context, None)
            }
            ActionPayload::Aggregated { action_name, matches, template } => {
                let monitor_name = matches.first().map(|m| m.monitor_name.clone());
                let context = serde_json::json!({
                    "matches": matches,
                    "monitor_name": monitor_name,
                });
                (action_name, context, Some(template))
            }
        };

        tracing::info!(action = %action_name, "Executing notification action.");

        let action_config = self.actions.get(&action_name).ok_or_else(|| {
            tracing::warn!(action = %action_name, "Action configuration not found.");
            ActionDispatcherError::ConfigError(format!("Action '{action_name}' not found"))
        })?;
        tracing::debug!(action = %action_name, "Found action configuration.");

        // Handle different action types separately
        match &action_config.config {
            // Standard output action
            ActionTypeConfig::Stdout(stdout_config) =>
                self.execute_stdout(stdout_config, &context)?,
            // All webhook-based actions are handled here
            ActionTypeConfig::Discord(_)
            | ActionTypeConfig::Slack(_)
            | ActionTypeConfig::Telegram(_)
            | ActionTypeConfig::Webhook(_) =>
                self.execute_webhook(action_config, &context, custom_template).await?,
        }

        Ok(())
    }

    /// Executes a webhook notification.
    async fn execute_webhook(
        &self,
        action_config: &ActionConfig,
        context: &serde_json::Value,
        custom_template: Option<NotificationMessage>,
    ) -> Result<(), ActionDispatcherError> {
        let action_name = &action_config.name;
        // Use the AsWebhookComponents trait to get config, retry policy and payload
        // builder
        let mut components = action_config.config.as_webhook_components()?;

        // If a custom template is provided (e.g., for aggregation), override the
        // default one.
        if let Some(template) = custom_template {
            components.config.title = template.title;
            components.config.body_template = template.body;
        }

        // Get or create the HTTP client from the pool based on the retry policy
        let http_client = self.client_pool.get_or_create(&components.retry_policy).await?;

        // Render the title and body templates.
        let rendered_title =
            self.template_service.render(&components.config.title, context.clone())?;
        let rendered_body =
            self.template_service.render(&components.config.body_template, context.clone())?;
        tracing::debug!(action = %action_name, body = %rendered_body, "Rendered notification template.");

        // Build the payload
        let payload = components.builder.build_payload(&rendered_title, &rendered_body);

        // Create the action
        tracing::info!(action = %action_name, url = %components.config.url, "Dispatching notification.");
        let action = WebhookClient::new(components.config, http_client)?;

        action.notify_json(&payload).await?;
        tracing::info!(action = %action_name, "Notification dispatched successfully.");

        Ok(())
    }

    /// Executes a stdout notification.
    fn execute_stdout(
        &self,
        stdout_config: &StdoutConfig,
        context: &serde_json::Value,
    ) -> Result<(), ActionDispatcherError> {
        if let Some(message) = &stdout_config.message {
            let rendered_title = self.template_service.render(&message.title, context.clone())?;
            let rendered_body = self.template_service.render(&message.body, context.clone())?;

            println!("=== Stdout Notification: ===\n{}\n{}\n", rendered_title, rendered_body);
            Ok(())
        } else {
            println!("=== Stdout Notification: ===\n {}", context);
            Ok(())
        }
    }

    /// Runs the notification service, listening for incoming monitor matches
    /// and executing notifications based on the configured actions.
    pub async fn run(&self, mut notifications_rx: mpsc::Receiver<MonitorMatch>) {
        while let Some(monitor_match) = notifications_rx.recv().await {
            if let Err(e) = self.execute(ActionPayload::Single(monitor_match.clone())).await {
                tracing::error!(
                    "Failed to execute notification for action '{}': {}",
                    monitor_match.action_name,
                    e
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::{TxHash, address};
    use serde_json::json;
    use url::Url;

    use super::*;
    use crate::{
        config::HttpRetryConfig,
        models::{
            action::{DiscordConfig, GenericWebhookConfig, SlackConfig, TelegramConfig},
            monitor_match::LogDetails,
            notification::NotificationMessage,
        },
        test_helpers::ActionBuilder,
    };

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
        )
    }

    #[tokio::test]
    async fn test_missing_action_error() {
        let http_client_pool = Arc::new(HttpClientPool::default());
        let service = ActionDispatcher::new(Arc::new(HashMap::new()), http_client_pool);
        let monitor_match = create_mock_monitor_match("nonexistent");
        let notification_payload = ActionPayload::Single(monitor_match.clone());
        let result = service.execute(notification_payload).await;

        assert!(result.is_err());
        match result {
            Err(ActionDispatcherError::ConfigError(msg)) => {
                assert!(msg.contains("Action 'nonexistent' not found"));
            }
            _ => panic!("Expected ConfigError"),
        }
    }

    #[test]
    fn as_webhook_components_trait_for_slack_config() {
        let title = "Slack Title";
        let message = "Slack Body";
        let url = Url::parse("https://slack.example.com").unwrap();

        let slack_config = ActionTypeConfig::Slack(SlackConfig {
            slack_url: url.clone(),
            message: NotificationMessage { title: title.to_string(), body: message.to_string() },
            retry_policy: HttpRetryConfig::default(),
        });

        let components = slack_config.as_webhook_components().unwrap();

        // Assert WebhookConfig is correct
        assert_eq!(components.config.url, url);
        assert_eq!(components.config.title, title);
        assert_eq!(components.config.body_template, message);
        assert_eq!(components.config.method, Some("POST".to_string()));
        assert!(components.config.secret.is_none());

        // Assert the builder creates the correct payload
        let payload = components.builder.build_payload(title, message);
        assert!(payload.get("blocks").is_some(), "Expected a Slack payload with 'blocks'");
        assert!(payload.get("content").is_none(), "Did not expect a Discord payload");
    }

    #[test]
    fn as_webhook_components_trait_for_discord_config() {
        let title = "Discord Title"; // Not directly used in Discord payload, but part of the message struct
        let message = "Discord Body";
        let url = Url::parse("https://discord.example.com").unwrap();

        let discord_config = ActionTypeConfig::Discord(DiscordConfig {
            discord_url: url.clone(),
            message: NotificationMessage { title: title.to_string(), body: message.to_string() },
            retry_policy: HttpRetryConfig::default(),
        });

        let components = discord_config.as_webhook_components().unwrap();

        // Assert WebhookConfig is correct
        assert_eq!(components.config.url, url);
        assert_eq!(components.config.title, title);
        assert_eq!(components.config.body_template, message);
        assert_eq!(components.config.method, Some("POST".to_string()));
        assert!(components.config.secret.is_none());

        // Assert the builder creates the correct payload
        let payload = components.builder.build_payload(title, message);
        assert_eq!(payload.get("content").unwrap(), &format!("*{title}*\n\n{message}"));
        assert!(payload.get("blocks").is_none(), "Did not expect a Slack payload");
    }

    #[test]
    fn as_webhook_components_trait_for_telegram_config() {
        let title = "Telegram Title"; // Not used in Telegram payload
        let message = "Telegram Body";
        let token = "test_token";
        let chat_id = "test_chat_id";

        let telegram_config = ActionTypeConfig::Telegram(TelegramConfig {
            token: token.to_string(),
            chat_id: chat_id.to_string(),
            message: NotificationMessage { title: title.to_string(), body: message.to_string() },
            disable_web_preview: Some(true),
            retry_policy: HttpRetryConfig::default(),
        });

        let components = telegram_config.as_webhook_components().unwrap();

        // Assert WebhookConfig is correct
        assert_eq!(
            components.config.url,
            Url::parse(&format!("https://api.telegram.org/bot{token}/sendMessage")).unwrap()
        );
        assert_eq!(components.config.title, title);
        assert_eq!(components.config.body_template, message);
        assert_eq!(components.config.method, Some("POST".to_string()));

        // Assert the builder creates the correct payload
        let payload = components.builder.build_payload(title, message);
        assert_eq!(payload.get("chat_id").unwrap(), chat_id);
        assert_eq!(payload.get("text").unwrap(), &format!("*{title}* \n\n{message}"));
        assert_eq!(payload.get("disable_web_page_preview").unwrap(), &json!(true));
    }

    #[test]
    fn as_webhook_components_trait_for_generic_webhook_config() {
        let title = "Webhook Title";
        let message = "Webhook Body";
        let url = Url::parse("https://webhook.example.com").unwrap();
        let mut headers = HashMap::new();
        headers.insert("X-Test-Header".to_string(), "Value".to_string());

        let webhook_config = ActionTypeConfig::Webhook(GenericWebhookConfig {
            url: url.clone(),
            message: NotificationMessage { title: title.to_string(), body: message.to_string() },
            method: Some("PUT".to_string()),
            secret: Some("my-secret".to_string()),
            headers: Some(headers.clone()),
            retry_policy: HttpRetryConfig::default(),
        });

        let components = webhook_config.as_webhook_components().unwrap();

        // Assert WebhookConfig is correct
        assert_eq!(components.config.url, url);
        assert_eq!(components.config.method, Some("PUT".to_string()));
        assert_eq!(components.config.secret, Some("my-secret".to_string()));
        assert_eq!(components.config.headers, Some(headers));

        // Assert the builder creates the correct payload
        let payload = components.builder.build_payload(title, message);
        assert_eq!(payload.get("title").unwrap(), title);
        assert_eq!(payload.get("body").unwrap(), message);
    }

    #[test]
    fn as_webhook_components_trait_fails_for_stdout_config() {
        let stdout_config = ActionTypeConfig::Stdout(StdoutConfig { message: None });

        let result = stdout_config.as_webhook_components();

        assert!(result.is_err());
        match result {
            Err(ActionDispatcherError::ConfigError(msg)) => {
                assert!(msg.contains("Stdout action does not support webhook components"));
            }
            _ => panic!("Expected ConfigError"),
        }
    }

    #[test]
    fn test_execute_stdout_with_message() {
        let action_config = ActionBuilder::new("stdout_test")
            .stdout_config(Some(NotificationMessage {
                title: "Test Title".to_string(),
                body: "This is a test body.".to_string(),
            }))
            .build();

        let stdout_config = match &action_config.config {
            ActionTypeConfig::Stdout(c) => c,
            _ => panic!("Expected StdoutConfig"),
        };

        let context = serde_json::json!({
            "monitor_name": "Test Monitor",
            "block_number": 123,
            "transaction_hash": "0xabc123",
            "tx": {
                "from": "0xfromaddress",
                "to": "0xtoaddress",
                "value": 1000
            }
        });

        let service = ActionDispatcher::new(
            Arc::new(
                vec![(action_config.name.clone(), action_config.clone())].into_iter().collect(),
            ),
            Arc::new(HttpClientPool::default()),
        );

        let result = service.execute_stdout(stdout_config, &context);

        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_stdout_without_message() {
        let action_config = ActionBuilder::new("stdout_test").stdout_config(None).build();

        let stdout_config = match &action_config.config {
            ActionTypeConfig::Stdout(c) => c,
            _ => panic!("Expected StdoutConfig"),
        };

        let context = serde_json::json!({
            "monitor_name": "Test Monitor",
            "block_number": 123,
            "transaction_hash": "0xabc123",
            "tx": {
                "from": "0xfromaddress",
                "to": "0xtoaddress",
                "value": 1000
            }
        });

        let service = ActionDispatcher::new(
            Arc::new(
                vec![(action_config.name.clone(), action_config.clone())].into_iter().collect(),
            ),
            Arc::new(HttpClientPool::default()),
        );

        let result = service.execute_stdout(stdout_config, &context);

        assert!(result.is_ok());
    }
}
