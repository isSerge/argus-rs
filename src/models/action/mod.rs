//! This module defines the data structures for action configurations.

mod kafka;
mod nats;
mod policies;
mod rabbitmq;
mod stdout;
mod webhook;

pub use kafka::{KafkaConfig, KafkaProducerConfig, KafkaSecurityConfig};
pub use nats::{NatsConfig, NatsCredentials};
pub use policies::{ActionPolicy, AggregationPolicy, ThrottlePolicy};
pub use rabbitmq::RabbitMqConfig;
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
    /// A RabbitMQ event publisher.
    RabbitMq(RabbitMqConfig),
    /// A NATS event publisher.
    Nats(NatsConfig),
}

/// Error types for Action configuration validation.
#[derive(Debug, Clone, Error)]
pub enum ActionTypeConfigError {
    /// Error for empty title in webhook message.
    #[error("Webhook title cannot be empty.")]
    EmptyTitle,

    /// Error for invalid HTTP/HTTPS URL in a webhook message.
    #[error("Webhook URL must be valid HTTP or HTTPS")]
    InvalidWebhookURL,

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
    EmptyKafkaTopic,

    /// Error for empty Kafka publisher brokers.
    #[error("Event publisher brokers cannot be empty.")]
    EmptyKafkaBrokers,

    /// Error for invalid Kafka publisher broker host:port pairs.
    #[error("Event publisher brokers cannot be empty.")]
    InvalidKafkaBrokerConfig,

    /// Error for empty RabbitMQ URI.
    #[error("RabbitMQ URI cannot be empty.")]
    EmptyRabbitMqUri,

    /// Error for invalid RabbitMQ URI.
    #[error("Invalid RabbitMQ URI: must be a valid AMQP or AMQPS URI.")]
    InvalidRabbitMqUri,

    /// Error for empty RabbitMQ exchange.
    #[error("RabbitMQ exchange cannot be empty.")]
    EmptyRabbitMqExchange,

    /// Error for empty NATS server URLs.
    #[error("NATS server URLs cannot be empty.")]
    EmptyNatsUrls,

    /// Error for invalid NATS server URLs.
    #[error("NATS server URLs must be valid.")]
    InvalidNatsUrls,

    /// Error for empty NATS subject.
    #[error("NATS subject cannot be empty.")]
    EmptyNatsSubject,

    /// Error for providing both token and file for NATS credentials.
    #[error("NATS credentials cannot have both token and file set.")]
    BothNatsCredentials,
}

impl ActionTypeConfig {
    /// Validates the Action configuration.
    pub fn validate(&self) -> Result<(), ActionTypeConfigError> {
        match self {
            ActionTypeConfig::Webhook(config) => {
                if config.message.title.is_empty() {
                    return Err(ActionTypeConfigError::EmptyTitle);
                }
                if config.url.scheme() != "http" && config.url.scheme() != "https" {
                    return Err(ActionTypeConfigError::InvalidWebhookURL);
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

            // Kafka publisher validation.
            ActionTypeConfig::Kafka(config) => {
                if config.topic.is_empty() {
                    return Err(ActionTypeConfigError::EmptyKafkaTopic);
                }

                if config.brokers.is_empty() {
                    return Err(ActionTypeConfigError::EmptyKafkaBrokers);
                }

                // Each broker should be a valid host:port pair
                for broker in config.brokers.split(',').map(|s| s.trim()).filter(|s| !s.is_empty())
                {
                    let mut parts = broker.split(':');
                    let host = parts.next();
                    let port = parts.next();
                    // There should be exactly one ':'
                    if host.is_none() || port.is_none() || parts.next().is_some() {
                        return Err(ActionTypeConfigError::InvalidKafkaBrokerConfig);
                    }
                    // Optionally, validate host (non-empty) and port (parse as u16)
                    if host.unwrap().is_empty() {
                        return Err(ActionTypeConfigError::InvalidKafkaBrokerConfig);
                    }
                    let port_str = port.unwrap();
                    if port_str.parse::<u16>().is_err() {
                        return Err(ActionTypeConfigError::InvalidKafkaBrokerConfig);
                    }
                }

                Ok(())
            }

            // RabbitMQ publisher validation.
            ActionTypeConfig::RabbitMq(config) => {
                if config.uri.is_empty() {
                    return Err(ActionTypeConfigError::EmptyRabbitMqUri);
                }
                // Validate that it's a valid URI with amqp:// or amqps:// scheme
                if !matches!(url::Url::parse(&config.uri), Ok(uri) if uri.scheme() == "amqp" || uri.scheme() == "amqps")
                {
                    return Err(ActionTypeConfigError::InvalidRabbitMqUri);
                }

                if config.exchange.is_empty() {
                    return Err(ActionTypeConfigError::EmptyRabbitMqExchange);
                }

                Ok(())
            }

            // NATS publisher validation.
            ActionTypeConfig::Nats(config) => {
                if config.urls.is_empty() {
                    return Err(ActionTypeConfigError::EmptyNatsUrls);
                }
                for url_str in config.urls.split(",") {
                    let url_str = url_str.trim();
                    if url_str.is_empty() {
                        return Err(ActionTypeConfigError::EmptyNatsUrls);
                    }
                    if !matches!(url::Url::parse(url_str), Ok(uri) if uri.scheme() == "nats") {
                        return Err(ActionTypeConfigError::InvalidNatsUrls);
                    }
                }

                if config.subject.is_empty() {
                    return Err(ActionTypeConfigError::EmptyNatsSubject);
                }

                if let Some(creds) = &config.credentials
                    && creds.token.is_some()
                    && creds.file.is_some()
                {
                    return Err(ActionTypeConfigError::BothNatsCredentials);
                }

                Ok(())
            }
        }
    }
}

/// Represents a single Action configuration from the YAML file.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActionConfig {
    /// The unique ID of the Action.
    /// Optional because it may not be set until persisted to the database.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
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
    fn test_validate_webhook_invalid_url_scheme() {
        let config = ActionTypeConfig::Webhook(GenericWebhookConfig {
            url: Url::parse("ftp://localhost/webhook").unwrap(),
            message: notification_message(),
            method: None,
            secret: None,
            headers: None,
            retry_policy: HttpRetryConfig::default(),
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ActionTypeConfigError::InvalidWebhookURL));
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
        assert!(matches!(result.unwrap_err(), ActionTypeConfigError::EmptyKafkaTopic));
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
        assert!(matches!(result.unwrap_err(), ActionTypeConfigError::EmptyKafkaBrokers));
    }

    #[test]
    fn test_validate_rabbitmq_ok() {
        let config = ActionTypeConfig::RabbitMq(RabbitMqConfig {
            uri: "amqp://guest:guest@localhost:5672/%2f".to_string(),
            exchange: "test_exchange".to_string(),
            ..Default::default()
        });
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_rabbitmq_empty_uri() {
        let config = ActionTypeConfig::RabbitMq(RabbitMqConfig {
            uri: "".to_string(),
            exchange: "test_exchange".to_string(),
            ..Default::default()
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ActionTypeConfigError::EmptyRabbitMqUri));
    }
    #[test]
    fn test_validate_rabbitmq_invalid_uri_scheme() {
        let config = ActionTypeConfig::RabbitMq(RabbitMqConfig {
            uri: "http://localhost:5672".to_string(),
            exchange: "test_exchange".to_string(),
            ..Default::default()
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ActionTypeConfigError::InvalidRabbitMqUri));
    }

    #[test]
    fn test_validate_rabbitmq_empty_exchange() {
        let config = ActionTypeConfig::RabbitMq(RabbitMqConfig {
            uri: "amqp://guest:guest@localhost:5672/%2f".to_string(),
            exchange: "".to_string(),
            ..Default::default()
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ActionTypeConfigError::EmptyRabbitMqExchange));
    }

    #[test]
    fn test_validate_nats_ok() {
        let config = ActionTypeConfig::Nats(NatsConfig {
            urls: "nats://localhost:4222, nats://localhost:4223".to_string(),
            subject: "test_subject".to_string(),
            ..Default::default()
        });
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_nats_empty_urls() {
        let config = ActionTypeConfig::Nats(NatsConfig {
            urls: "".to_string(),
            subject: "test_subject".to_string(),
            ..Default::default()
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ActionTypeConfigError::EmptyNatsUrls));
    }

    #[test]
    fn test_validate_nats_invalid_url_scheme() {
        let config = ActionTypeConfig::Nats(NatsConfig {
            urls: "amqp://localhost:4222".to_string(),
            subject: "test_subject".to_string(),
            ..Default::default()
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ActionTypeConfigError::InvalidNatsUrls));
    }

    #[test]
    fn test_validate_nats_empty_subject() {
        let config = ActionTypeConfig::Nats(NatsConfig {
            urls: "nats://localhost:4222, nats://localhost:4223".to_string(),
            subject: "".to_string(),
            ..Default::default()
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ActionTypeConfigError::EmptyNatsSubject));
    }

    #[test]
    fn test_validate_nats_with_credentials_token() {
        let config = ActionTypeConfig::Nats(NatsConfig {
            urls: "nats://localhost:4222, nats://localhost:4223".to_string(),
            subject: "test_subject".to_string(),
            credentials: Some(NatsCredentials {
                token: Some("test_token".to_string()),
                file: None,
            }),
        });
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_nats_with_credentials_file() {
        let config = ActionTypeConfig::Nats(NatsConfig {
            urls: "nats://localhost:4222, nats://localhost:4223".to_string(),
            subject: "test_subject".to_string(),
            credentials: Some(NatsCredentials { token: None, file: Some("test_file".to_string()) }),
        });
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_nats_with_both_credentials() {
        let config = ActionTypeConfig::Nats(NatsConfig {
            urls: "nats://localhost:4222, nats://localhost:4223".to_string(),
            subject: "test_subject".to_string(),
            credentials: Some(NatsCredentials {
                token: Some("test_token".to_string()),
                file: Some("test_file".to_string()),
            }),
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ActionTypeConfigError::BothNatsCredentials));
    }
}
