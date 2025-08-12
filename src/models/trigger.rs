//! This module defines the data structures for trigger configurations.

use crate::config::HttpRetryConfig;
use crate::models::notification::NotificationMessage;
use crate::notification::error::NotificationError;
use serde::Deserialize;
use std::collections::HashMap;
use url::Url;

/// Configuration for a generic webhook.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
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
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
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
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
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
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
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
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
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

impl TriggerTypeConfig {
    /// Validates the trigger configuration.
    pub fn validate(&self) -> Result<(), NotificationError> {
        match self {
            TriggerTypeConfig::Webhook(config) => {
                if Url::parse(&config.url).is_err() {
                    return Err(NotificationError::ConfigError(format!(
                        "Invalid webhook URL: {}",
                        config.url
                    )));
                }
                if config.message.title.is_empty() {
                    return Err(NotificationError::ConfigError(
                        "Webhook title cannot be empty.".to_string(),
                    ));
                }
                Ok(())
            }
            TriggerTypeConfig::Slack(config) => {
                if Url::parse(&config.slack_url).is_err() {
                    return Err(NotificationError::ConfigError(format!(
                        "Invalid Slack URL: {}",
                        config.slack_url
                    )));
                }
                if !config.slack_url.starts_with("https://hooks.slack.com/") {
                    return Err(NotificationError::ConfigError(
                        "Slack URL appears to be invalid.".to_string(),
                    ));
                }
                Ok(())
            }
            TriggerTypeConfig::Discord(config) => {
                if Url::parse(&config.discord_url).is_err() {
                    return Err(NotificationError::ConfigError(format!(
                        "Invalid Discord URL: {}",
                        config.discord_url
                    )));
                }
                Ok(())
            }
            TriggerTypeConfig::Telegram(config) => {
                if config.token.is_empty() {
                    return Err(NotificationError::ConfigError(
                        "Telegram token cannot be empty.".to_string(),
                    ));
                }
                if config.chat_id.is_empty() {
                    return Err(NotificationError::ConfigError(
                        "Telegram chat_id cannot be empty.".to_string(),
                    ));
                }
                Ok(())
            }
        }
    }
}

/// Represents a single trigger configuration from the YAML file.
#[derive(Debug, Clone, Deserialize)]
pub struct TriggerConfig {
    /// The unique name of the trigger.
    pub name: String,
    /// The specific configuration for the trigger type.
    #[serde(flatten)]
    pub config: TriggerTypeConfig,
}