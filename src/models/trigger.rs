//! This module defines the data structures for trigger configurations.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

use crate::{config::HttpRetryConfig, models::notification::NotificationMessage};

/// Configuration for a generic webhook.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
pub struct WebhookConfig {
    /// The URL of the webhook endpoint.
    pub url: String,
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
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
pub struct SlackConfig {
    /// The Slack webhook URL.
    pub slack_url: String,
    /// The message content for the notification.
    pub message: NotificationMessage,
    /// The retry policy configuration for HTTP requests.
    #[serde(default)]
    pub retry_policy: HttpRetryConfig,
}

/// Configuration for a Discord notification.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
pub struct DiscordConfig {
    /// The Discord webhook URL.
    pub discord_url: String,
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

/// An enum representing the different types of trigger configurations.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TriggerTypeConfig {
    /// A generic webhook trigger.
    Webhook(WebhookConfig),
    /// A Slack notification trigger.
    Slack(SlackConfig),
    /// A Discord notification trigger.
    Discord(DiscordConfig),
    /// A Telegram notification trigger.
    Telegram(TelegramConfig),
}

/// Error types for trigger configuration validation.
#[derive(Debug, Clone, Error)]
pub enum TriggerTypeConfigError {
    /// Error for invalid URL formats.
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// Error for empty title in webhook message.
    #[error("Webhook title cannot be empty.")]
    EmptyTitle,

    /// Error for empty Telegram token.
    #[error("Telegram token cannot be empty.")]
    EmptyTelegramToken,

    /// Error for empty Telegram chat ID.
    #[error("Telegram chat ID cannot be empty.")]
    EmptyTelegramChatId,
}

impl TriggerTypeConfig {
    /// Validates the trigger configuration.
    pub fn validate(&self) -> Result<(), TriggerTypeConfigError> {
        match self {
            TriggerTypeConfig::Webhook(config) => {
                if Url::parse(&config.url).is_err() {
                    return Err(TriggerTypeConfigError::InvalidUrl(config.url.clone()));
                }
                if config.message.title.is_empty() {
                    return Err(TriggerTypeConfigError::EmptyTitle);
                }
                Ok(())
            }
            TriggerTypeConfig::Slack(config) => {
                if Url::parse(&config.slack_url).is_err() {
                    return Err(TriggerTypeConfigError::InvalidUrl(config.slack_url.clone()));
                }
                if !config.slack_url.starts_with("https://hooks.slack.com/") {
                    return Err(TriggerTypeConfigError::InvalidUrl(config.slack_url.clone()));
                }
                Ok(())
            }
            TriggerTypeConfig::Discord(config) => {
                if Url::parse(&config.discord_url).is_err() {
                    return Err(TriggerTypeConfigError::InvalidUrl(config.discord_url.clone()));
                }
                Ok(())
            }
            TriggerTypeConfig::Telegram(config) => {
                if config.token.is_empty() {
                    return Err(TriggerTypeConfigError::EmptyTelegramToken);
                }
                if config.chat_id.is_empty() {
                    return Err(TriggerTypeConfigError::EmptyTelegramChatId);
                }
                Ok(())
            }
        }
    }
}

/// Represents a single trigger configuration from the YAML file.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TriggerConfig {
    /// The unique name of the trigger.
    pub name: String,
    /// The specific configuration for the trigger type.
    #[serde(flatten)]
    pub config: TriggerTypeConfig,
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
        let config = TriggerTypeConfig::Webhook(WebhookConfig {
            url: "http://localhost/webhook".to_string(),
            message: notification_message(),
            ..Default::default()
        });
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_webhook_invalid_url() {
        let config = TriggerTypeConfig::Webhook(WebhookConfig {
            url: "not a url".to_string(),
            message: notification_message(),
            ..Default::default()
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TriggerTypeConfigError::InvalidUrl(_)));
    }

    #[test]
    fn test_validate_webhook_empty_title() {
        let config = TriggerTypeConfig::Webhook(WebhookConfig {
            url: "http://localhost/webhook".to_string(),
            message: NotificationMessage { title: "".to_string(), body: "Test Body".to_string() },
            ..Default::default()
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TriggerTypeConfigError::EmptyTitle));
    }

    #[test]
    fn test_validate_slack_ok() {
        let config = TriggerTypeConfig::Slack(SlackConfig {
            slack_url:
                "https://hooks.slack.com/services/T00000000/B00000000/XXXXXXXXXXXXXXXXXXXXXXXX"
                    .to_string(),
            message: notification_message(),
            ..Default::default()
        });
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_slack_invalid_url() {
        let config = TriggerTypeConfig::Slack(SlackConfig {
            slack_url: "not a url".to_string(),
            message: notification_message(),
            ..Default::default()
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TriggerTypeConfigError::InvalidUrl(_)));
    }

    #[test]
    fn test_validate_slack_not_a_slack_url() {
        let config = TriggerTypeConfig::Slack(SlackConfig {
            slack_url: "https://example.com/not-slack".to_string(),
            message: notification_message(),
            ..Default::default()
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TriggerTypeConfigError::InvalidUrl(_)));
    }

    #[test]
    fn test_validate_discord_ok() {
        let config = TriggerTypeConfig::Discord(DiscordConfig {
            discord_url: "https://discord.com/api/webhooks/1234567890/abcdef".to_string(),
            message: notification_message(),
            ..Default::default()
        });
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_discord_invalid_url() {
        let config = TriggerTypeConfig::Discord(DiscordConfig {
            discord_url: "not a url".to_string(),
            message: notification_message(),
            ..Default::default()
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TriggerTypeConfigError::InvalidUrl(_)));
    }

    #[test]
    fn test_validate_telegram_ok() {
        let config = TriggerTypeConfig::Telegram(TelegramConfig {
            token: "test_token".to_string(),
            chat_id: "test_chat_id".to_string(),
            message: notification_message(),
            ..Default::default()
        });
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_telegram_empty_token() {
        let config = TriggerTypeConfig::Telegram(TelegramConfig {
            token: "".to_string(),
            chat_id: "test_chat_id".to_string(),
            message: notification_message(),
            ..Default::default()
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TriggerTypeConfigError::EmptyTelegramToken));
    }

    #[test]
    fn test_validate_telegram_empty_chat_id() {
        let config = TriggerTypeConfig::Telegram(TelegramConfig {
            token: "test_token".to_string(),
            chat_id: "".to_string(),
            message: notification_message(),
            ..Default::default()
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TriggerTypeConfigError::EmptyTelegramChatId));
    }
}
