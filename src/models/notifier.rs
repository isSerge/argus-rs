//! This module defines the data structures for notifier configurations.

use std::{collections::HashMap, time::Duration};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

use crate::{
    config::{HttpRetryConfig, deserialize_duration_from_seconds, serialize_duration_to_seconds},
    loader::{Loadable, LoaderError},
    models::notification::NotificationMessage,
};

/// Configuration for a generic webhook.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct WebhookConfig {
    /// The URL of the webhook endpoint.
    pub url: Url,
    /// The HTTP method to use for the webhook (e.g., "POST", "GET").
    pub method: Option<String>,
    /// An optional secret for signing webhook requests.
    pub secret: Option<String>,
    /// Optional custom headers to include in the webhook request.
    pub headers: Option<HashMap<String, String>>,
    /// The message content for the notification.
    pub message: NotificationMessage,
    /// The retry policy configuration for HTTP requests.
    #[serde(default)]
    pub retry_policy: HttpRetryConfig,
}

/// Configuration for a Slack notification.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SlackConfig {
    /// The Slack webhook URL.
    pub slack_url: Url,
    /// The message content for the notification.
    pub message: NotificationMessage,
    /// The retry policy configuration for HTTP requests.
    #[serde(default)]
    pub retry_policy: HttpRetryConfig,
}

/// Configuration for a Discord notification.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct DiscordConfig {
    /// The Discord webhook URL.
    pub discord_url: Url,
    /// The message content for the notification.
    pub message: NotificationMessage,
    /// The retry policy configuration for HTTP requests.
    #[serde(default)]
    pub retry_policy: HttpRetryConfig,
}

/// Configuration for a Telegram notification.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
pub struct TelegramConfig {
    /// The Telegram bot token.
    pub token: String,
    /// The chat ID to send the message to.
    pub chat_id: String,
    /// The message content for the notification.
    pub message: NotificationMessage,
    /// Whether to disable web page preview for the message.
    pub disable_web_preview: Option<bool>,
    /// The retry policy configuration for HTTP requests.
    #[serde(default)]
    pub retry_policy: HttpRetryConfig,
}

/// Configuration for a Stdout notification.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
pub struct StdoutConfig {
    /// The optional message content for the notification.
    /// If not provided, the full event payload will be serialized to JSON.
    pub message: Option<NotificationMessage>,
}

/// The type of notifier configuration.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotifierTypeConfig {
    /// A generic webhook.
    Webhook(WebhookConfig),
    /// A Slack notification.
    Slack(SlackConfig),
    /// A Discord notification.
    Discord(DiscordConfig),
    /// A Telegram notification.
    Telegram(TelegramConfig),
    /// A stdout notification.
    Stdout(StdoutConfig),
}

/// Error types for notifier configuration validation.
#[derive(Debug, Clone, Error)]
pub enum NotifierTypeConfigError {
    /// Error for empty title in webhook message.
    #[error("Webhook title cannot be empty.")]
    EmptyTitle,

    /// Error for empty Telegram token.
    #[error("Telegram token cannot be empty.")]
    EmptyTelegramToken,

    /// Error for empty Telegram chat ID.
    #[error("Telegram chat ID cannot be empty.")]
    EmptyTelegramChatId,

    /// Error for invalid Discord webhook URL.
    #[error("Invalid Discord URL: must be a valid Discord webhook URL.")]
    InvalidDiscordUrl,

    /// Error for invalid Slack webhook URL.
    #[error("Invalid Slack URL: must be a valid Slack webhook URL.")]
    InvalidSlackUrl,
}

impl NotifierTypeConfig {
    /// Validates the notifier configuration.
    pub fn validate(&self) -> Result<(), NotifierTypeConfigError> {
        match self {
            NotifierTypeConfig::Webhook(config) => {
                if config.message.title.is_empty() {
                    return Err(NotifierTypeConfigError::EmptyTitle);
                }
                Ok(())
            }
            NotifierTypeConfig::Slack(config) => {
                if config.slack_url.domain() != Some("hooks.slack.com") {
                    return Err(NotifierTypeConfigError::InvalidSlackUrl);
                }
                Ok(())
            }
            NotifierTypeConfig::Discord(config) => {
                if config.discord_url.domain() != Some("discord.com") {
                    return Err(NotifierTypeConfigError::InvalidDiscordUrl);
                }
                Ok(())
            }
            NotifierTypeConfig::Telegram(config) => {
                if config.token.is_empty() {
                    return Err(NotifierTypeConfigError::EmptyTelegramToken);
                }
                if config.chat_id.is_empty() {
                    return Err(NotifierTypeConfigError::EmptyTelegramChatId);
                }
                Ok(())
            }
            // Standard output notifier requires no validation.
            NotifierTypeConfig::Stdout(_) => Ok(()),
        }
    }
}

/// Represents a single notifier configuration from the YAML file.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NotifierConfig {
    /// The unique name of the notifier.
    pub name: String,

    /// The specific configuration for the notifier type.
    #[serde(flatten)]
    pub config: NotifierTypeConfig,

    /// Optional policy for handling notifications (e.g., aggregation,
    /// throttling).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<NotifierPolicy>,
}

/// Notification policies for handling notifications
#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum NotifierPolicy {
    /// Policy for aggregating multiple notifications into a single one.
    Aggregation(AggregationPolicy),

    /// Policy for throttling notifications to avoid spamming.
    Throttle(ThrottlePolicy),
}

/// Policy for aggregating multiple notifications into a single one.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct AggregationPolicy {
    /// The time window in seconds for the aggregation policy.
    #[serde(
        deserialize_with = "deserialize_duration_from_seconds",
        serialize_with = "serialize_duration_to_seconds"
    )]
    pub window_secs: Duration,

    /// The template to use for the aggregated notification message.
    pub template: NotificationMessage,
}

/// Policy for throttling notifications to avoid spamming.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ThrottlePolicy {
    /// The maximum number of notifications to send within the specified time
    /// window.
    pub max_count: u32,

    /// The time window in seconds for the throttling policy.
    #[serde(
        deserialize_with = "deserialize_duration_from_seconds",
        serialize_with = "serialize_duration_to_seconds"
    )]
    pub time_window_secs: Duration,
}

/// Errors that can occur during notifier processing.
#[derive(Debug, Error)]
pub enum NotifierError {
    /// An error occurred during the loading process.
    #[error("Failed to load notifier configuration.")]
    Loader(#[from] LoaderError),

    /// An error occurred during validation.
    #[error("Failed to validate notifier configuration.")]
    Validation(#[from] NotifierTypeConfigError),
}

impl Loadable for NotifierConfig {
    type Error = NotifierError;

    const KEY: &'static str = "notifiers";

    fn validate(&mut self) -> Result<(), Self::Error> {
        self.config.validate().map_err(NotifierError::Validation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::notification::NotificationMessage;

    // Helper to create a default notification message
    fn notification_message() -> NotificationMessage {
        NotificationMessage { title: "Test Title".to_string(), body: "Test Body".to_string() }
    }

    #[test]
    fn test_validate_webhook_ok() {
        let config = NotifierTypeConfig::Webhook(WebhookConfig {
            url: Url::parse("http://localhost/webhook").unwrap(),
            message: notification_message(),
            method: None,
            secret: None,
            headers: None,
            retry_policy: HttpRetryConfig::default(),
        });
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_webhook_empty_title() {
        let config = NotifierTypeConfig::Webhook(WebhookConfig {
            url: Url::parse("http://localhost/webhook").unwrap(),
            message: NotificationMessage { title: "".to_string(), body: "Test Body".to_string() },
            method: None,
            secret: None,
            headers: None,
            retry_policy: HttpRetryConfig::default(),
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), NotifierTypeConfigError::EmptyTitle));
    }

    #[test]
    fn test_validate_slack_ok() {
        let config = NotifierTypeConfig::Slack(SlackConfig {
            slack_url: Url::parse(
                "https://hooks.slack.com/services/T00000000/B00000000/XXXXXXXXXXXXXXXXXXXXXXXX",
            )
            .unwrap(),
            message: notification_message(),
            retry_policy: HttpRetryConfig::default(),
        });
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_slack_not_a_slack_url() {
        let config = NotifierTypeConfig::Slack(SlackConfig {
            slack_url: Url::parse("https://example.com/not-slack").unwrap(),
            message: notification_message(),
            retry_policy: HttpRetryConfig::default(),
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), NotifierTypeConfigError::InvalidSlackUrl));
    }

    #[test]
    fn test_validate_discord_ok() {
        let config = NotifierTypeConfig::Discord(DiscordConfig {
            discord_url: Url::parse("https://discord.com/api/webhooks/1234567890/abcdef").unwrap(),
            message: notification_message(),
            retry_policy: HttpRetryConfig::default(),
        });
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_discord_invalid_url() {
        let config = NotifierTypeConfig::Discord(DiscordConfig {
            discord_url: Url::parse("https://example.com/not-discord").unwrap(),
            message: notification_message(),
            retry_policy: HttpRetryConfig::default(),
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), NotifierTypeConfigError::InvalidDiscordUrl));
    }

    #[test]
    fn test_validate_telegram_ok() {
        let config = NotifierTypeConfig::Telegram(TelegramConfig {
            token: "test_token".to_string(),
            chat_id: "test_chat_id".to_string(),
            message: notification_message(),
            ..Default::default()
        });
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_telegram_empty_token() {
        let config = NotifierTypeConfig::Telegram(TelegramConfig {
            token: "".to_string(),
            chat_id: "test_chat_id".to_string(),
            message: notification_message(),
            ..Default::default()
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), NotifierTypeConfigError::EmptyTelegramToken));
    }

    #[test]
    fn test_validate_telegram_empty_chat_id() {
        let config = NotifierTypeConfig::Telegram(TelegramConfig {
            token: "test_token".to_string(),
            chat_id: "".to_string(),
            message: notification_message(),
            ..Default::default()
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), NotifierTypeConfigError::EmptyTelegramChatId));
    }

    #[test]
    fn test_validate_stdout_ok() {
        let config = NotifierTypeConfig::Stdout(StdoutConfig { message: None });
        assert!(config.validate().is_ok());
    }
}
