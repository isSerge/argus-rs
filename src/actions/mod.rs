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
    actions::{stdout::StdoutAction, traits::Action, webhook::WebhookAction},
    http_client::HttpClientPool,
    models::{
        action::{ActionConfig, ActionTypeConfig},
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

use self::template::TemplateService;

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
    // /// A thread-safe pool for creating and reusing HTTP clients with different
    // /// retry policies.
    // client_pool: Arc<HttpClientPool>,
    // /// A map of action names to their loaded and validated configurations.
    // actions: Arc<HashMap<String, ActionConfig>>,
    // /// The service for rendering notification templates.
    // template_service: TemplateService,
    actions: HashMap<String, Box<dyn Action>>,
}

impl ActionDispatcher {
    /// Creates a new `ActionDispatcher` instance.
    ///
    /// # Arguments
    ///
    /// * `actions` - A vector of `ActionConfig` loaded and validated at
    ///   application startup.
    /// * `client_pool` - A shared pool of HTTP clients.
    pub async fn new(
        action_configs: Arc<HashMap<String, ActionConfig>>,
        client_pool: Arc<HttpClientPool>,
    ) -> Result<Self, ActionDispatcherError> {
        let template_service = Arc::new(TemplateService::new());
        let mut actions: HashMap<String, Box<dyn Action>> = HashMap::new();

        for (name, config) in action_configs.iter() {
            let action: Box<dyn Action> = match &config.config {
                // Standard output action
                ActionTypeConfig::Stdout(c) =>
                    Box::new(StdoutAction::new(c.clone(), template_service.clone())),
                // All webhook-based actions are constructed here
                ActionTypeConfig::Webhook(_)
                | ActionTypeConfig::Discord(_)
                | ActionTypeConfig::Slack(_)
                | ActionTypeConfig::Telegram(_) => {
                    // This unwrap is safe because we've already filtered non-webhook types
                    let components = config.config.as_webhook_components().unwrap();
                    let http_client = client_pool.get_or_create(&components.retry_policy).await?;
                    Box::new(WebhookAction::new(components, http_client, template_service.clone()))
                }
            };
            actions.insert(name.clone(), action);
        }

        Ok(ActionDispatcher { actions })
    }

    /// Executes a notification for a given action.
    pub async fn execute(&self, payload: ActionPayload) -> Result<(), ActionDispatcherError> {
        let action_name = payload.action_name();

        tracing::debug!(action = %action_name, "Executing action.");

        let action = &self.actions.get(&action_name).ok_or_else(|| {
            ActionDispatcherError::ConfigError(format!("Action '{}' not found", action_name))
        })?;

        action.execute(payload).await
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
            action::{
                DiscordConfig, GenericWebhookConfig, SlackConfig, StdoutConfig, TelegramConfig,
            },
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
        let service =
            ActionDispatcher::new(Arc::new(HashMap::new()), http_client_pool).await.unwrap();
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

    #[tokio::test]
    async fn test_execute_stdout_with_message() {
        let action_config = ActionBuilder::new("stdout_test")
            .stdout_config(Some(NotificationMessage {
                title: "Test Title".to_string(),
                body: "This is a test body.".to_string(),
            }))
            .build();

        let action_payload = ActionPayload::Single(create_mock_monitor_match(&action_config.name));

        let service = ActionDispatcher::new(
            Arc::new(
                vec![(action_config.name.clone(), action_config.clone())].into_iter().collect(),
            ),
            Arc::new(HttpClientPool::default()),
        )
        .await
        .unwrap();

        let result = service.execute(action_payload).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_stdout_without_message() {
        let action_config = ActionBuilder::new("stdout_test").stdout_config(None).build();

        let action_payload = ActionPayload::Single(create_mock_monitor_match(&action_config.name));

        let service = ActionDispatcher::new(
            Arc::new(
                vec![(action_config.name.clone(), action_config.clone())].into_iter().collect(),
            ),
            Arc::new(HttpClientPool::default()),
        )
        .await
        .unwrap();

        let result = service.execute(action_payload).await;

        assert!(result.is_ok());
    }
}
