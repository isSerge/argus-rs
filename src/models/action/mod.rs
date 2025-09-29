//! This module defines the data structures for action configurations.

mod kafka;
mod policies;
mod stdout;
mod webhook;

pub use kafka::KafkaConfig;
pub use policies::{ActionPolicy, AggregationPolicy, ThrottlePolicy};
use serde::{Deserialize, Serialize};
pub use stdout::StdoutConfig;
use thiserror::Error;
pub use webhook::{DiscordConfig, GenericWebhookConfig, SlackConfig, TelegramConfig};

use crate::loader::{Loadable, LoaderError};

/// The type of action configuration.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionTypeConfig {
    /// A generic webhook.
    Webhook(GenericWebhookConfig),
    /// A Slack notification.
    Slack(SlackConfig),
    /// A Discord notification.
    Discord(DiscordConfig),
    /// A Telegram notification.
    Telegram(TelegramConfig),
    /// A stdout notification.
    Stdout(StdoutConfig),
    /// A Kafka event publisher.
    Kafka(KafkaConfig),
}

/// Error types for Action configuration validation.
#[derive(Debug, Clone, Error)]
pub enum ActionTypeConfigError {
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

    /// Error for empty Kafka publisher topic.
    #[error("Event publisher topic cannot be empty.")]
    EmptyPublisherTopic,

    /// Error for empty Kafka publisher brokers.
    #[error("Event publisher brokers cannot be empty.")]
    EmptyPublisherBrokers,
}

impl ActionTypeConfig {
    /// Validates the Action configuration.
    pub fn validate(&self) -> Result<(), ActionTypeConfigError> {
        match self {
            ActionTypeConfig::Webhook(config) => {
                if config.message.title.is_empty() {
                    return Err(ActionTypeConfigError::EmptyTitle);
                }
                Ok(())
            }
            ActionTypeConfig::Slack(config) => {
                if config.slack_url.domain() != Some("hooks.slack.com") {
                    return Err(ActionTypeConfigError::InvalidSlackUrl);
                }
                Ok(())
            }
            ActionTypeConfig::Discord(config) => {
                if config.discord_url.domain() != Some("discord.com") {
                    return Err(ActionTypeConfigError::InvalidDiscordUrl);
                }
                Ok(())
            }
            ActionTypeConfig::Telegram(config) => {
                if config.token.is_empty() {
                    return Err(ActionTypeConfigError::EmptyTelegramToken);
                }
                if config.chat_id.is_empty() {
                    return Err(ActionTypeConfigError::EmptyTelegramChatId);
                }
                Ok(())
            }
            // Standard output Action requires no validation.
            ActionTypeConfig::Stdout(_) => Ok(()),

            ActionTypeConfig::Kafka(config) => {
                if config.topic.is_empty() {
                    return Err(ActionTypeConfigError::EmptyPublisherTopic);
                }

                if config.brokers.is_empty() {
                    return Err(ActionTypeConfigError::EmptyPublisherBrokers);
                }

                Ok(())
            }
        }
    }
}

/// Represents a single Action configuration from the YAML file.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActionConfig {
    /// The unique name of the Action.
    pub name: String,

    /// The specific configuration for the Action type.
    #[serde(flatten)]
    pub config: ActionTypeConfig,

    /// Optional policy for handling notifications (e.g., aggregation,
    /// throttling).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<ActionPolicy>,
}

/// Errors that can occur during Action processing.
#[derive(Debug, Error)]
pub enum ActionConfigError {
    /// An error occurred during the loading process.
    #[error("Failed to load Action configuration.")]
    Loader(#[from] LoaderError),

    /// An error occurred during validation.
    #[error("Failed to validate Action configuration.")]
    Validation(#[from] ActionTypeConfigError),
}

impl Loadable for ActionConfig {
    type Error = ActionConfigError;

    const KEY: &'static str = "actions";

    fn validate(&mut self) -> Result<(), Self::Error> {
        self.config.validate().map_err(ActionConfigError::Validation)
    }
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::*;
    use crate::{config::HttpRetryConfig, models::notification::NotificationMessage};

    // Helper to create a default notification message
    fn notification_message() -> NotificationMessage {
        NotificationMessage { title: "Test Title".to_string(), body: "Test Body".to_string() }
    }

    #[test]
    fn test_validate_webhook_ok() {
        let config = ActionTypeConfig::Webhook(GenericWebhookConfig {
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
        let config = ActionTypeConfig::Webhook(GenericWebhookConfig {
            url: Url::parse("http://localhost/webhook").unwrap(),
            message: NotificationMessage { title: "".to_string(), body: "Test Body".to_string() },
            method: None,
            secret: None,
            headers: None,
            retry_policy: HttpRetryConfig::default(),
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ActionTypeConfigError::EmptyTitle));
    }

    #[test]
    fn test_validate_slack_ok() {
        let config = ActionTypeConfig::Slack(SlackConfig {
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
        let config = ActionTypeConfig::Slack(SlackConfig {
            slack_url: Url::parse("https://example.com/not-slack").unwrap(),
            message: notification_message(),
            retry_policy: HttpRetryConfig::default(),
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ActionTypeConfigError::InvalidSlackUrl));
    }

    #[test]
    fn test_validate_discord_ok() {
        let config = ActionTypeConfig::Discord(DiscordConfig {
            discord_url: Url::parse("https://discord.com/api/webhooks/1234567890/abcdef").unwrap(),
            message: notification_message(),
            retry_policy: HttpRetryConfig::default(),
        });
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_discord_invalid_url() {
        let config = ActionTypeConfig::Discord(DiscordConfig {
            discord_url: Url::parse("https://example.com/not-discord").unwrap(),
            message: notification_message(),
            retry_policy: HttpRetryConfig::default(),
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ActionTypeConfigError::InvalidDiscordUrl));
    }

    #[test]
    fn test_validate_telegram_ok() {
        let config = ActionTypeConfig::Telegram(TelegramConfig {
            token: "test_token".to_string(),
            chat_id: "test_chat_id".to_string(),
            message: notification_message(),
            ..Default::default()
        });
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_telegram_empty_token() {
        let config = ActionTypeConfig::Telegram(TelegramConfig {
            token: "".to_string(),
            chat_id: "test_chat_id".to_string(),
            message: notification_message(),
            ..Default::default()
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ActionTypeConfigError::EmptyTelegramToken));
    }

    #[test]
    fn test_validate_telegram_empty_chat_id() {
        let config = ActionTypeConfig::Telegram(TelegramConfig {
            token: "test_token".to_string(),
            chat_id: "".to_string(),
            message: notification_message(),
            ..Default::default()
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ActionTypeConfigError::EmptyTelegramChatId));
    }

    #[test]
    fn test_validate_stdout_ok() {
        let config = ActionTypeConfig::Stdout(StdoutConfig { message: None });
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_kafka_ok() {
        let config = ActionTypeConfig::Kafka(KafkaConfig {
            brokers: "localhost:9092, localhost:9093".to_string(),
            topic: "test_topic".to_string(),
            ..Default::default()
        });
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_kafka_empty_topic() {
        let config = ActionTypeConfig::Kafka(KafkaConfig {
            brokers: "localhost:9092, localhost:9093".to_string(),
            topic: "".to_string(),
            ..Default::default()
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ActionTypeConfigError::EmptyPublisherTopic));
    }

    #[test]
    fn test_validate_kafka_empty_brokers() {
        let config = ActionTypeConfig::Kafka(KafkaConfig {
            brokers: "".to_string(),
            topic: "test_topic".to_string(),
            ..Default::default()
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ActionTypeConfigError::EmptyPublisherBrokers));
    }
}
