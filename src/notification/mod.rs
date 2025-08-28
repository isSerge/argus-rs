//! # Notification Service
//!
//! This module is responsible for sending notifications through various
//! webhook-based channels based on notifier configurations. It acts as the
//! central hub for dispatching alerts when a monitor finds a match.
//!
//! ## Core Components
//!
//! - **`NotificationService`**: The main struct that holds the loaded notifier
//!   configurations and a shared `HttpClientPool`. It is responsible for
//!   executing notifications.
//! - **`AsWebhookComponents` Trait**: A helper trait that abstracts the
//!   specific details of each notification channel (Slack, Discord, etc.) into
//!   a common `WebhookComponents` struct. This allows the service to handle
//!   different webhook providers polymorphically.
//! - **Payload Builders**: Located in the `payload_builder` module, these are
//!   responsible for constructing the JSON payload specific to each
//!   notification channel.
//!
//! ## Workflow
//!
//! 1. The `NotificationService` is initialized with a collection of validated
//!    `NotifierConfig`s, which are loaded at application startup.
//! 2. When a monitor match occurs, the `execute` method is called with the name
//!    of the notifier to be executed and a set of variables for template
//!    substitution.
//! 3. The service looks up the corresponding `NotifierTypeConfig` by its name.
//! 4. The `as_webhook_components` trait method is called to transform the
//!    specific notifier configuration (e.g., `SlackConfig`) into a generic set
//!    of components needed to send a webhook.
//! 5. An HTTP client is retrieved from the `HttpClientPool`, configured with
//!    the appropriate retry policy for the specific notifier.
//! 6. The appropriate `WebhookPayloadBuilder` constructs the final JSON
//!    payload.
//! 7. The `WebhookNotifier` sends the request to the provider's endpoint.

use std::{collections::HashMap, sync::Arc};

use crate::{
    config::HttpRetryConfig,
    http_client::HttpClientPool,
    models::{
        monitor_match::MonitorMatch,
        notifier::{NotifierConfig, NotifierTypeConfig},
    },
};

pub mod error;
pub mod payload_builder;
mod template;
mod webhook;

use error::NotificationError;
use payload_builder::{
    DiscordPayloadBuilder, GenericWebhookPayloadBuilder, SlackPayloadBuilder,
    TelegramPayloadBuilder, WebhookPayloadBuilder,
};
use tokio::sync::mpsc;

use self::{template::TemplateService, webhook::WebhookNotifier};

/// A private container struct holding the generic components required to send
/// any webhook-based notification.
///
/// This struct is created by the `AsWebhookComponents` trait and provides a
/// common interface for the `NotificationService` to work with, regardless of
/// the underlying provider (e.g., Slack, Discord).
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

/// A trait to convert a specific notifier configuration (e.g., `SlackConfig`)
/// into a generic `WebhookComponents` struct.
///
/// This abstraction allows the `NotificationService` to handle various webhook
/// providers in a uniform way, decoupling the core service logic from the
/// specific details of each notification channel.
trait AsWebhookComponents {
    /// Transforms a specific notifier configuration into the generic components
    /// needed to dispatch a webhook notification.
    ///
    /// This method extracts the URL, message, retry policy, and the appropriate
    /// payload builder from the specific config.
    fn as_webhook_components(&self) -> Result<WebhookComponents, NotificationError>;
}
impl AsWebhookComponents for NotifierTypeConfig {
    fn as_webhook_components(&self) -> Result<WebhookComponents, NotificationError> {
        // A private helper struct to hold the provider-specific details extracted from
        // the `match` statement. This helps reduce code repetition.
        struct ProviderDetails {
            url: String,
            builder: Box<dyn WebhookPayloadBuilder>,
            method: Option<String>,
            secret: Option<String>,
            headers: Option<HashMap<String, String>>,
        }

        impl ProviderDetails {
            /// A constructor for standard notifiers (like Slack, Discord) that
            /// use POST and have no custom secret or headers.
            fn new_standard(url: String, builder: Box<dyn WebhookPayloadBuilder>) -> Self {
                Self { url, builder, method: Some("POST".to_string()), secret: None, headers: None }
            }
        }

        // Extract common components (message and retry_policy) that exist in all
        // variants.
        let (message, retry_policy) = match self {
            NotifierTypeConfig::Webhook(c) => (&c.message, &c.retry_policy),
            NotifierTypeConfig::Discord(c) => (&c.message, &c.retry_policy),
            NotifierTypeConfig::Telegram(c) => (&c.message, &c.retry_policy),
            NotifierTypeConfig::Slack(c) => (&c.message, &c.retry_policy),
        };

        // The match statement now focuses only on creating the provider-specific
        // details.
        let details = match self {
            // The generic webhook is fully customizable.
            NotifierTypeConfig::Webhook(c) => ProviderDetails {
                url: c.url.clone(),
                builder: Box::new(GenericWebhookPayloadBuilder),
                method: c.method.clone(),
                secret: c.secret.clone(),
                headers: c.headers.clone(),
            },
            // Other notifiers use the standard POST configuration.
            NotifierTypeConfig::Discord(c) => ProviderDetails::new_standard(
                c.discord_url.clone(),
                Box::new(DiscordPayloadBuilder),
            ),
            NotifierTypeConfig::Telegram(c) => {
                let url = format!("https://api.telegram.org/bot{}/sendMessage", c.token);
                let builder = Box::new(TelegramPayloadBuilder {
                    chat_id: c.chat_id.clone(),
                    disable_web_preview: c.disable_web_preview.unwrap_or(false),
                });
                ProviderDetails::new_standard(url, builder)
            }
            NotifierTypeConfig::Slack(c) =>
                ProviderDetails::new_standard(c.slack_url.clone(), Box::new(SlackPayloadBuilder)),
        };

        // Construct the final WebhookConfig from the common and provider-specific
        // parts.
        let config = webhook::WebhookConfig {
            url: details.url,
            title: message.title.clone(),
            body_template: message.body.clone(),
            method: details.method,
            secret: details.secret,
            headers: details.headers,
            url_params: None,
        };

        Ok(WebhookComponents {
            config,
            retry_policy: retry_policy.clone(),
            builder: details.builder,
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
    notifiers: HashMap<String, NotifierConfig>,
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
    pub fn new(notifiers: Vec<NotifierConfig>, client_pool: Arc<HttpClientPool>) -> Self {
        let notifiers = notifiers.into_iter().map(|t| (t.name.clone(), t)).collect();
        NotificationService { client_pool, notifiers, template_service: TemplateService::new() }
    }

    /// Executes a notification for a given notifier.
    ///
    /// This method looks up the notifier by name, constructs the necessary
    /// components, and dispatches the notification.
    ///
    /// # Arguments
    ///
    /// * `monitor_match` - The monitor match data that initiated this notifier.
    ///
    /// # Returns
    ///
    /// * `Result<(), NotificationError>` - Returns `Ok(())` on success, or a
    ///   `NotificationError` if the notifier is not found, the HTTP client
    ///   fails, or the notification fails to send.
    pub async fn execute(&self, monitor_match: &MonitorMatch) -> Result<(), NotificationError> {
        let notifier_name = &monitor_match.notifier_name;
        tracing::info!(notifier = %notifier_name, "Executing notification notifier.");

        let notifier_config = self.notifiers.get(notifier_name).ok_or_else(|| {
            tracing::warn!(notifier = %notifier_name, "Notifier configuration not found.");
            NotificationError::ConfigError(format!("Notifier '{notifier_name}' not found"))
        })?;
        tracing::debug!(notifier = %notifier_name, "Found notifier configuration.");

        // Use the AsWebhookComponents trait to get config, retry policy and payload
        // builder
        let components = notifier_config.config.as_webhook_components()?;

        // Get or create the HTTP client from the pool based on the retry policy
        let http_client = self.client_pool.get_or_create(&components.retry_policy).await?;

        // Serialize the MonitorMatch to a JSON value for the template context.
        let context = serde_json::to_value(monitor_match).map_err(|e| {
            NotificationError::InternalError(format!("Failed to serialize monitor match: {e}"))
        })?;

        // Render the body template.
        let rendered_body =
            self.template_service.render(&components.config.body_template, context)?;
        tracing::debug!(notifier = %notifier_name, body = %rendered_body, "Rendered notification template.");

        // Build the payload
        let payload = components.builder.build_payload(&components.config.title, &rendered_body);

        // Create the notifier
        tracing::info!(notifier = %notifier_name, url = %components.config.url, "Dispatching notification.");
        let notifier = WebhookNotifier::new(components.config, http_client)?;

        notifier.notify_json(&payload).await?;
        tracing::info!(notifier = %notifier_name, "Notification dispatched successfully.");

        Ok(())
    }

    /// Runs the notification service, listening for incoming monitor matches
    /// and executing notifications based on the configured notifiers.
    pub async fn run(&self, mut notifications_rx: mpsc::Receiver<MonitorMatch>) {
        while let Some(monitor_match) = notifications_rx.recv().await {
            if let Err(e) = self.execute(&monitor_match).await {
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
    };

    fn create_mock_monitor_match(notifier_name: &str) -> MonitorMatch {
        let log_details = LogDetails {
            contract_address: address!("0x1234567890abcdef1234567890abcdef12345678"),
            log_index: 15,
            log_name: "TestLog".to_string(),
            log_params: json!({"param1": "value1", "param2": 42}),
        };
        MonitorMatch::new_log_match(
            1,
            "test monitor".to_string(),
            notifier_name.to_string(),
            123,
            TxHash::default(),
            log_details,
        )
    }

    #[tokio::test]
    async fn test_missing_notifier_error() {
        let http_client_pool = Arc::new(HttpClientPool::new());
        let service = NotificationService::new(vec![], http_client_pool);
        let monitor_match = create_mock_monitor_match("nonexistent");

        let result = service.execute(&monitor_match).await;

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

        let slack_config = NotifierTypeConfig::Slack(SlackConfig {
            slack_url: "https://slack.example.com".to_string(),
            message: NotificationMessage { title: title.to_string(), body: message.to_string() },
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
        assert!(payload.get("blocks").is_some(), "Expected a Slack payload with 'blocks'");
        assert!(payload.get("content").is_none(), "Did not expect a Discord payload");
    }

    #[test]
    fn as_webhook_components_trait_for_discord_config() {
        let title = "Discord Title"; // Not directly used in Discord payload, but part of the message struct
        let message = "Discord Body";

        let discord_config = NotifierTypeConfig::Discord(DiscordConfig {
            discord_url: "https://discord.example.com".to_string(),
            message: NotificationMessage { title: title.to_string(), body: message.to_string() },
            retry_policy: HttpRetryConfig::default(),
        });

        let components = discord_config.as_webhook_components().unwrap();

        // Assert WebhookConfig is correct
        assert_eq!(components.config.url, "https://discord.example.com");
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
            format!("https://api.telegram.org/bot{token}/sendMessage")
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
        let mut headers = HashMap::new();
        headers.insert("X-Test-Header".to_string(), "Value".to_string());

        let webhook_config = NotifierTypeConfig::Webhook(WebhookConfig {
            url: "https://webhook.example.com".to_string(),
            message: NotificationMessage { title: title.to_string(), body: message.to_string() },
            method: Some("PUT".to_string()),
            secret: Some("my-secret".to_string()),
            headers: Some(headers.clone()),
            retry_policy: HttpRetryConfig::default(),
        });

        let components = webhook_config.as_webhook_components().unwrap();

        // Assert WebhookConfig is correct
        assert_eq!(components.config.url, "https://webhook.example.com");
        assert_eq!(components.config.method, Some("PUT".to_string()));
        assert_eq!(components.config.secret, Some("my-secret".to_string()));
        assert_eq!(components.config.headers, Some(headers));

        // Assert the builder creates the correct payload
        let payload = components.builder.build_payload(title, message);
        assert_eq!(payload.get("title").unwrap(), title);
        assert_eq!(payload.get("body").unwrap(), message);
    }
}
