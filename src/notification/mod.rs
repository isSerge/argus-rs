//! # Notification Service
//!
//! This module is responsible for sending notifications through various
//! channels based on notifier configurations. It acts as the central hub for
//! dispatching alerts when a monitor finds a match.
//!
//! ## Core Components
//!
//! - **`NotificationService`**: The main struct that holds the loaded notifier
//!   configurations and a shared `HttpClientPool`. It is responsible for
//!   executing notifications.
//! - **`Notifier` Trait**: A generic interface for all notification channels,
//!   allowing for a unified dispatch mechanism.
//!
//! ## Workflow
//!
//! 1. The `NotificationService` is initialized with a collection of validated
//!    `NotifierConfig`s, which are loaded at application startup.
//! 2. For each `NotifierConfig`, a corresponding `Notifier` implementation
//!    (e.g., `WebhookNotifierWrapper`, `StdoutNotifier`) is created and stored.
//! 3. When a monitor match occurs, the `execute` method is called with the name
//!    of the notifier to be executed.
//! 4. The manager looks up the corresponding `Notifier` trait object and calls
//!    its `notify` method with the appropriate payload.

use std::{collections::HashMap, sync::Arc};

use crate::{
    config::HttpRetryConfig,
    http_client::HttpClientPool,
    models::{
        NotificationMessage,
        monitor_match::MonitorMatch,
        notifier::{
            self, DiscordConfig, NotifierConfig, NotifierTypeConfig, SlackConfig, TelegramConfig,
            WebhookConfig,
        },
    },
};

pub mod error;
pub mod payload_builder;
pub mod template;
mod webhook;

use error::NotificationError;
use payload_builder::{
    DiscordPayloadBuilder, GenericWebhookPayloadBuilder, SlackPayloadBuilder,
    TelegramPayloadBuilder, WebhookPayloadBuilder,
};
use tokio::sync::mpsc;
use url::Url;

use self::{template::TemplateService, webhook::WebhookNotifier};

/// An enum representing the different types of notification payloads.
pub enum NotificationPayload {
    /// A single notification for a single monitor match.
    Single(MonitorMatch),
    /// An aggregated notification for multiple monitor matches.
    Aggregated {
        /// The name of the notifier to use for this aggregated notification.
        notifier_name: String,
        /// The list of monitor matches to include in the aggregation.
        matches: Vec<MonitorMatch>,
        /// The template to use for the notification message.
        template: NotificationMessage,
    },
}

/// A private container struct holding the generic components required to send
/// any webhook-based notification.
///
/// This struct provides a common interface for the `NotificationService` to
/// work with, regardless of the underlying provider (e.g., Slack, Discord).
struct WebhookComponents {
    /// The generic webhook configuration, including the URL, method, and
    /// headers.
    config: webhook::WebhookConfig,
    /// The specific retry policy for this notification channel.
    retry_policy: HttpRetryConfig,
    /// The payload builder responsible for creating the channel-specific JSON
    /// body.
    builder: Box<dyn WebhookPayloadBuilder>,
}

impl From<&WebhookConfig> for WebhookComponents {
    fn from(c: &WebhookConfig) -> Self {
        WebhookComponents {
            config: webhook::WebhookConfig {
                url: c.url.clone(),
                title: c.message.title.clone(),
                body_template: c.message.body.clone(),
                method: c.method.clone(),
                secret: c.secret.clone(),
                headers: c.headers.clone(),
                url_params: None,
            },
            retry_policy: c.retry_policy.clone(),
            builder: Box::new(GenericWebhookPayloadBuilder),
        }
    }
}

impl From<&DiscordConfig> for WebhookComponents {
    fn from(c: &DiscordConfig) -> Self {
        WebhookComponents {
            config: webhook::WebhookConfig {
                url: c.discord_url.clone(),
                title: c.message.title.clone(),
                body_template: c.message.body.clone(),
                method: Some("POST".to_string()),
                secret: None,
                headers: None,
                url_params: None,
            },
            retry_policy: c.retry_policy.clone(),
            builder: Box::new(DiscordPayloadBuilder),
        }
    }
}

impl From<&TelegramConfig> for WebhookComponents {
    fn from(c: &TelegramConfig) -> Self {
        WebhookComponents {
            config: webhook::WebhookConfig {
                url: Url::parse(&format!("https://api.telegram.org/bot{}/sendMessage", c.token))
                    .unwrap(),
                title: c.message.title.clone(),
                body_template: c.message.body.clone(),
                method: Some("POST".to_string()),
                secret: None,
                headers: None,
                url_params: None,
            },
            retry_policy: c.retry_policy.clone(),
            builder: Box::new(TelegramPayloadBuilder {
                chat_id: c.chat_id.clone(),
                disable_web_preview: c.disable_web_preview.unwrap_or(false),
            }),
        }
    }
}

impl From<&SlackConfig> for WebhookComponents {
    fn from(c: &SlackConfig) -> Self {
        WebhookComponents {
            config: webhook::WebhookConfig {
                url: c.slack_url.clone(),
                title: c.message.title.clone(),
                body_template: c.message.body.clone(),
                method: Some("POST".to_string()),
                secret: None,
                headers: None,
                url_params: None,
            },
            retry_policy: c.retry_policy.clone(),
            builder: Box::new(SlackPayloadBuilder),
        }
    }
}

impl NotifierTypeConfig {
    /// Transforms the specific notifier configuration into a generic set of
    /// webhook components.
    fn as_webhook_components(&self) -> Result<WebhookComponents, NotificationError> {
        Ok(match self {
            NotifierTypeConfig::Webhook(c) => c.into(),
            NotifierTypeConfig::Discord(c) => c.into(),
            NotifierTypeConfig::Telegram(c) => c.into(),
            NotifierTypeConfig::Slack(c) => c.into(),
            NotifierTypeConfig::Stdout(_) =>
                return Err(NotificationError::ConfigError(
                    "Stdout notifier does not support webhook components".to_string(),
                )),
        })
    }
}

/// A service responsible for dispatching notifications based on pre-loaded
/// notifier configurations.
pub struct NotificationService {
    /// A thread-safe pool for creating and reusing HTTP clients with different
    /// retry policies.
    client_pool: Arc<HttpClientPool>,
    /// A map of notifier names to their loaded and validated configurations.
    notifiers: Arc<HashMap<String, NotifierConfig>>,
    /// The service for rendering notification templates.
    template_service: TemplateService,
}

impl NotificationService {
    /// Creates a new `NotificationService` instance.
    ///
    /// # Arguments
    ///
    /// * `notifiers` - A vector of `NotifierConfig` loaded and validated at
    ///   application startup.
    /// * `client_pool` - A shared pool of HTTP clients.
    pub fn new(
        notifiers: Arc<HashMap<String, NotifierConfig>>,
        client_pool: Arc<HttpClientPool>,
    ) -> Self {
        NotificationService { client_pool, notifiers, template_service: TemplateService::new() }
    }

    /// Executes a notification for a given notifier.
    ///
    /// This method looks up the notifier by name, constructs the necessary
    /// components, and dispatches the notification.
    ///
    /// # Arguments
    ///
    /// * `payload` - The notification payload, which can be either a single
    ///   match or an aggregated summary.
    ///
    /// # Returns
    ///
    /// * `Result<(), NotificationError>` - Returns `Ok(())` on success, or a
    ///   `NotificationError` if the notifier is not found, the HTTP client
    ///   fails, or the notification fails to send.
    pub async fn execute(&self, payload: NotificationPayload) -> Result<(), NotificationError> {
        let (notifier_name, context, custom_template) = match payload {
            NotificationPayload::Single(monitor_match) => {
                let context = serde_json::to_value(&monitor_match).map_err(|e| {
                    NotificationError::InternalError(format!(
                        "Failed to serialize monitor match: {e}"
                    ))
                })?;
                (monitor_match.notifier_name, context, None)
            }
            NotificationPayload::Aggregated { notifier_name, matches, template } => {
                let monitor_name = matches.first().map(|m| m.monitor_name.clone());
                let context = serde_json::json!({
                    "matches": matches,
                    "monitor_name": monitor_name,
                });
                (notifier_name, context, Some(template))
            }
        };

        tracing::info!(notifier = %notifier_name, "Executing notification notifier.");

        let notifier_config = self.notifiers.get(&notifier_name).ok_or_else(|| {
            tracing::warn!(notifier = %notifier_name, "Notifier configuration not found.");
            NotificationError::ConfigError(format!("Notifier '{notifier_name}' not found"))
        })?;
        tracing::debug!(notifier = %notifier_name, "Found notifier configuration.");

        // Handle different notifier types separately
        match &notifier_config.config {
            // Standard output notifier
            NotifierTypeConfig::Stdout(stdout_config) =>
                self.execute_stdout(stdout_config, &context)?,
            // All webhook-based notifiers are handled here
            NotifierTypeConfig::Discord(_)
            | NotifierTypeConfig::Slack(_)
            | NotifierTypeConfig::Telegram(_)
            | NotifierTypeConfig::Webhook(_) =>
                self.execute_webhook(notifier_config, &context, custom_template).await?,
        }

        Ok(())
    }

    /// Executes a webhook notification.
    async fn execute_webhook(
        &self,
        notifier_config: &NotifierConfig,
        context: &serde_json::Value,
        custom_template: Option<NotificationMessage>,
    ) -> Result<(), NotificationError> {
        let notifier_name = &notifier_config.name;
        // Use the AsWebhookComponents trait to get config, retry policy and payload
        // builder
        let mut components = notifier_config.config.as_webhook_components()?;

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
        tracing::debug!(notifier = %notifier_name, body = %rendered_body, "Rendered notification template.");

        // Build the payload
        let payload = components.builder.build_payload(&rendered_title, &rendered_body);

        // Create the notifier
        tracing::info!(notifier = %notifier_name, url = %components.config.url, "Dispatching notification.");
        let notifier = WebhookNotifier::new(components.config, http_client)?;

        notifier.notify_json(&payload).await?;
        tracing::info!(notifier = %notifier_name, "Notification dispatched successfully.");

        Ok(())
    }

    /// Executes a stdout notification.
    fn execute_stdout(
        &self,
        stdout_config: &notifier::StdoutConfig,
        context: &serde_json::Value,
    ) -> Result<(), NotificationError> {
        if let Some(message) = &stdout_config.message {
            let rendered_title = self.template_service.render(&message.title, context.clone())?;
            let rendered_body = self.template_service.render(&message.body, context.clone())?;

            println!("=== Stdout Notification: ===\n{}\n{}\n", rendered_title, rendered_body);
            return Ok(());
        } else {
            println!("=== Stdout Notification: ===\n {}\n", context);
            return Ok(());
        }
    }

    /// Runs the notification service, listening for incoming monitor matches
    /// and executing notifications based on the configured notifiers.
    pub async fn run(&self, mut notifications_rx: mpsc::Receiver<MonitorMatch>) {
        while let Some(monitor_match) = notifications_rx.recv().await {
            if let Err(e) = self.execute(NotificationPayload::Single(monitor_match.clone())).await {
                tracing::error!(
                    "Failed to execute notification for notifier '{}': {}",
                    monitor_match.notifier_name,
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

    use super::*;
    use crate::{
        config::HttpRetryConfig,
        models::{
            monitor_match::LogDetails,
            notification::NotificationMessage,
            notifier::{DiscordConfig, SlackConfig, TelegramConfig, WebhookConfig},
        },
        test_helpers::NotifierBuilder,
    };

    fn create_mock_monitor_match(notifier_name: &str) -> MonitorMatch {
        let log_details = LogDetails {
            address: address!("0x1234567890abcdef1234567890abcdef12345678"),
            log_index: 15,
            name: "TestLog".to_string(),
            params: json!({"param1": "value1", "param2": 42}),
        };
        MonitorMatch::new_log_match(
            1,
            "test monitor".to_string(),
            notifier_name.to_string(),
            123,
            TxHash::default(),
            log_details,
            json!({}),
        )
    }

    #[tokio::test]
    async fn test_missing_notifier_error() {
        let http_client_pool = Arc::new(HttpClientPool::default());
        let service = NotificationService::new(Arc::new(HashMap::new()), http_client_pool);
        let monitor_match = create_mock_monitor_match("nonexistent");
        let notification_payload = NotificationPayload::Single(monitor_match.clone());
        let result = service.execute(notification_payload).await;

        assert!(result.is_err());
        match result {
            Err(NotificationError::ConfigError(msg)) => {
                assert!(msg.contains("Notifier 'nonexistent' not found"));
            }
            _ => panic!("Expected ConfigError"),
        }
    }

    #[test]
    fn as_webhook_components_trait_for_slack_config() {
        let title = "Slack Title";
        let message = "Slack Body";
        let url = Url::parse("https://slack.example.com").unwrap();

        let slack_config = NotifierTypeConfig::Slack(SlackConfig {
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

        let discord_config = NotifierTypeConfig::Discord(DiscordConfig {
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

        let telegram_config = NotifierTypeConfig::Telegram(TelegramConfig {
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

        let webhook_config = NotifierTypeConfig::Webhook(WebhookConfig {
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
        let stdout_config = NotifierTypeConfig::Stdout(notifier::StdoutConfig { message: None });

        let result = stdout_config.as_webhook_components();

        assert!(result.is_err());
        match result {
            Err(NotificationError::ConfigError(msg)) => {
                assert!(msg.contains("Stdout notifier does not support webhook components"));
            }
            _ => panic!("Expected ConfigError"),
        }
    }

    #[test]
    fn test_execute_stdout_with_message() {
        let notifier_config = NotifierBuilder::new("stdout_test")
            .stdout_config(Some(NotificationMessage {
                title: "Test Title".to_string(),
                body: "This is a test body.".to_string(),
            }))
            .build();

        let stdout_config = match &notifier_config.config {
            NotifierTypeConfig::Stdout(c) => c,
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

        let service = NotificationService::new(
            Arc::new(
                vec![(notifier_config.name.clone(), notifier_config.clone())].into_iter().collect(),
            ),
            Arc::new(HttpClientPool::default()),
        );

        let result = service.execute_stdout(stdout_config, &context);

        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_stdout_without_message() {
        let notifier_config = NotifierBuilder::new("stdout_test").stdout_config(None).build();

        let stdout_config = match &notifier_config.config {
            NotifierTypeConfig::Stdout(c) => c,
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

        let service = NotificationService::new(
            Arc::new(
                vec![(notifier_config.name.clone(), notifier_config.clone())].into_iter().collect(),
            ),
            Arc::new(HttpClientPool::default()),
        );

        let result = service.execute_stdout(stdout_config, &context);

        assert!(result.is_ok());
    }
}
