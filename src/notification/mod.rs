//! # Notification Service
//!
//! This module is responsible for sending notifications through various webhook-based
//! channels based on trigger configurations. It acts as the central hub for dispatching
//! alerts when a monitor finds a match.
//!
//! ## Core Components
//!
//! - **`NotificationService`**: The main struct that holds the loaded trigger
//!   configurations and a shared `HttpClientPool`. It is responsible for executing
//!   notifications.
//! - **`AsWebhookComponents` Trait**: A helper trait that abstracts the specific details
//!   of each notification channel (Slack, Discord, etc.) into a common `WebhookComponents`
//!   struct. This allows the service to handle different webhook providers polymorphically.
//! - **Payload Builders**: Located in the `payload_builder` module, these are responsible
//!   for constructing the JSON payload specific to each notification channel.
//!
//! ## Workflow
//!
//! 1. The `NotificationService` is initialized with a collection of validated
//!    `TriggerConfig`s, which are loaded at application startup.
//! 2. When a monitor match occurs, the `execute` method is called with the name of the
//!    trigger to be executed and a set of variables for template substitution.
//! 3. The service looks up the corresponding `TriggerTypeConfig` by its name.
//! 4. The `as_webhook_components` trait method is called to transform the specific
//!    trigger configuration (e.g., `SlackConfig`) into a generic set of components
//!    needed to send a webhook.
//! 5. An HTTP client is retrieved from the `HttpClientPool`, configured with the
//!    appropriate retry policy for the specific trigger.
//! 6. The appropriate `WebhookPayloadBuilder` constructs the final JSON payload.
//! 7. The `WebhookNotifier` sends the request to the provider's endpoint.

use std::{collections::HashMap, sync::Arc};

use crate::{
    config::HttpRetryConfig,
    http_client::HttpClientPool,
    models::{
        monitor_match::MonitorMatch,
        trigger::{
            DiscordConfig, SlackConfig, TelegramConfig, TriggerConfig, TriggerTypeConfig,
            WebhookConfig,
        },
    },
};

pub mod error;
pub mod payload_builder;
mod template;
mod webhook;

use self::{template::TemplateService, webhook::WebhookNotifier};
use error::NotificationError;
use payload_builder::{
    DiscordPayloadBuilder, GenericWebhookPayloadBuilder, SlackPayloadBuilder,
    TelegramPayloadBuilder, WebhookPayloadBuilder,
};
use tokio::sync::mpsc;

/// A private container struct holding the generic components required to send any
/// webhook-based notification.
///
/// This struct is created by the `AsWebhookComponents` trait and provides a common
/// interface for the `NotificationService` to work with, regardless of the underlying
/// provider (e.g., Slack, Discord).
struct WebhookComponents {
    /// The generic webhook configuration, including the URL, method, and headers.
    config: webhook::WebhookConfig,
    /// The specific retry policy for this notification channel.
    retry_policy: HttpRetryConfig,
    /// The payload builder responsible for creating the channel-specific JSON body.
    builder: Box<dyn WebhookPayloadBuilder>,
}

/// A trait to convert a specific trigger configuration (e.g., `SlackConfig`) into
/// a generic `WebhookComponents` struct.
///
/// This abstraction allows the `NotificationService` to handle various webhook providers
/// in a uniform way, decoupling the core service logic from the specific details of
/// each notification channel.
trait AsWebhookComponents {
    /// Transforms a specific trigger configuration into the generic components needed
    /// to dispatch a webhook notification.
    ///
    /// This method extracts the URL, message, retry policy, and the appropriate
    /// payload builder from the specific config.
    fn as_webhook_components(&self) -> Result<WebhookComponents, NotificationError>;
}

impl AsWebhookComponents for TriggerTypeConfig {
    fn as_webhook_components(&self) -> Result<WebhookComponents, NotificationError> {
        let (url, message, method, secret, headers, builder, retry_policy): (
            _,
            _,
            _,
            _,
            _,
            _,
            HttpRetryConfig,
        ) = match self {
            TriggerTypeConfig::Webhook(WebhookConfig {
                url,
                message,
                method,
                secret,
                headers,
                retry_policy,
            }) => (
                url.clone(),
                message.clone(),
                method.clone(),
                secret.clone(),
                headers.clone(),
                Box::new(GenericWebhookPayloadBuilder) as Box<dyn WebhookPayloadBuilder>,
                retry_policy.clone(),
            ),
            TriggerTypeConfig::Discord(DiscordConfig {
                discord_url,
                message,
                retry_policy,
            }) => (
                discord_url.clone(),
                message.clone(),
                Some("POST".to_string()),
                None,
                None,
                Box::new(DiscordPayloadBuilder),
                retry_policy.clone(),
            ),
            TriggerTypeConfig::Telegram(TelegramConfig {
                token,
                message,
                chat_id,
                disable_web_preview,
                retry_policy,
            }) => (
                format!("https://api.telegram.org/bot{token}/sendMessage"),
                message.clone(),
                Some("POST".to_string()),
                None,
                None,
                Box::new(TelegramPayloadBuilder {
                    chat_id: chat_id.clone(),
                    disable_web_preview: disable_web_preview.unwrap_or(false),
                }),
                retry_policy.clone(),
            ),
            TriggerTypeConfig::Slack(SlackConfig {
                slack_url,
                message,
                retry_policy,
            }) => (
                slack_url.clone(),
                message.clone(),
                Some("POST".to_string()),
                None,
                None,
                Box::new(SlackPayloadBuilder),
                retry_policy.clone(),
            ),
        };

        // Construct the final WebhookConfig from the extracted parts.
        let config = webhook::WebhookConfig {
            url,
            title: message.title,
            body_template: message.body,
            method,
            secret,
            headers,
            url_params: None,
        };

        Ok(WebhookComponents {
            config,
            retry_policy,
            builder,
        })
    }
}

/// A service responsible for dispatching notifications based on pre-loaded trigger configurations.
pub struct NotificationService {
    /// A thread-safe pool for creating and reusing HTTP clients with different retry policies.
    client_pool: Arc<HttpClientPool>,
    /// A map of trigger names to their loaded and validated configurations.
    triggers: HashMap<String, TriggerTypeConfig>,
    /// The service for rendering notification templates.
    template_service: TemplateService,
}

impl NotificationService {
    /// Creates a new `NotificationService` instance.
    ///
    /// # Arguments
    ///
    /// * `triggers` - A vector of `TriggerConfig` loaded and validated at application startup.
    /// * `client_pool` - A shared pool of HTTP clients.
    pub fn new(triggers: Vec<TriggerConfig>, client_pool: Arc<HttpClientPool>) -> Self {
        let triggers = triggers.into_iter().map(|t| (t.name, t.config)).collect();
        NotificationService {
            client_pool,
            triggers,
            template_service: TemplateService::new(),
        }
    }

    /// Executes a notification for a given trigger.
    ///
    /// This method looks up the trigger by name, constructs the necessary components,
    /// and dispatches the notification.
    ///
    /// # Arguments
    ///
    /// * `monitor_match` - The monitor match data that initiated this trigger.
    ///
    /// # Returns
    ///
    /// * `Result<(), NotificationError>` - Returns `Ok(())` on success, or a `NotificationError` if
    ///   the trigger is not found, the HTTP client fails, or the notification fails to send.
    pub async fn execute(&self, monitor_match: &MonitorMatch) -> Result<(), NotificationError> {
        let trigger_name = &monitor_match.trigger_name;
        let trigger_config = self.triggers.get(trigger_name).ok_or_else(|| {
            NotificationError::ConfigError(format!("Trigger '{trigger_name}' not found"))
        })?;

        // Use the AsWebhookComponents trait to get config, retry policy and payload builder
        let components = trigger_config.as_webhook_components()?;

        // Get or create the HTTP client from the pool based on the retry policy
        let http_client = self
            .client_pool
            .get_or_create(&components.retry_policy)
            .await?;

        // Serialize the MonitorMatch to a JSON value for the template context.
        let context = serde_json::to_value(monitor_match).map_err(|e| {
            NotificationError::InternalError(format!("Failed to serialize monitor match: {e}"))
        })?;

        // Render the body template.
        let rendered_body = self
            .template_service
            .render(&components.config.body_template, context)?;

        // Build the payload
        let payload = components
            .builder
            .build_payload(&components.config.title, &rendered_body);

        // Create the notifier
        let notifier = WebhookNotifier::new(components.config, http_client)?;

        notifier.notify_json(&payload).await?;

        Ok(())
    }

    /// Runs the notification service, listening for incoming monitor matches and executing
    /// notifications based on the configured triggers.
    pub async fn run(&self, mut notifications_rx: mpsc::Receiver<MonitorMatch>) {
        while let Some(monitor_match) = notifications_rx.recv().await {
            if let Err(e) = self.execute(&monitor_match).await {
                tracing::error!(
                    "Failed to execute notification for trigger '{}': {}",
                    monitor_match.trigger_name,
                    e
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::HttpRetryConfig, models::notification::NotificationMessage};
    use serde_json::json;

    fn create_mock_monitor_match(trigger_name: &str) -> MonitorMatch {
        MonitorMatch {
            monitor_id: 1,
            block_number: 123,
            transaction_hash: Default::default(),
            contract_address: Default::default(),
            trigger_name: trigger_name.to_string(),
            trigger_data: json!({ "foo": "bar" }),
            log_index: None,
        }
    }

    #[tokio::test]
    async fn test_missing_trigger_error() {
        let http_client_pool = Arc::new(HttpClientPool::new());
        let service = NotificationService::new(vec![], http_client_pool);
        let monitor_match = create_mock_monitor_match("nonexistent");

        let result = service.execute(&monitor_match).await;

        assert!(result.is_err());
        match result {
            Err(NotificationError::ConfigError(msg)) => {
                assert!(msg.contains("Trigger 'nonexistent' not found"));
            }
            _ => panic!("Expected ConfigError"),
        }
    }

    #[test]
    fn as_webhook_components_trait_for_slack_config() {
        let title = "Slack Title";
        let message = "Slack Body";

        let slack_config = TriggerTypeConfig::Slack(SlackConfig {
            slack_url: "https://slack.example.com".to_string(),
            message: NotificationMessage {
                title: title.to_string(),
                body: message.to_string(),
            },
            retry_policy: HttpRetryConfig::default(),
        });

        let components = slack_config.as_webhook_components().unwrap();

        // Assert WebhookConfig is correct
        assert_eq!(components.config.url, "https://slack.example.com");
        assert_eq!(components.config.title, title);
        assert_eq!(components.config.body_template, message);
        assert_eq!(components.config.method, Some("POST".to_string()));
        assert!(components.config.secret.is_none());

        // Assert the builder creates the correct payload
        let payload = components.builder.build_payload(title, message);
        assert!(
            payload.get("blocks").is_some(),
            "Expected a Slack payload with 'blocks'"
        );
        assert!(
            payload.get("content").is_none(),
            "Did not expect a Discord payload"
        );
    }
}
