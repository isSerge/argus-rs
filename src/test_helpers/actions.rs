use url::Url;

use crate::{
    config::HttpRetryConfig,
    models::{
        action::{
            ActionConfig, ActionPolicy, ActionTypeConfig, DiscordConfig, GenericWebhookConfig,
            KafkaConfig, NatsConfig, RabbitMqConfig, SlackConfig, StdoutConfig,
        },
        notification::NotificationMessage,
    },
};

/// A builder for creating `ActionConfig` instances for testing.
pub struct ActionBuilder {
    id: Option<i64>,
    name: String,
    config: ActionTypeConfig,
    policy: Option<ActionPolicy>,
}

impl ActionBuilder {
    /// Creates a new `ActionBuilder` with the given name.
    pub fn new(name: &str) -> Self {
        Self {
            id: None,
            name: name.to_string(),
            config: ActionTypeConfig::Webhook(GenericWebhookConfig {
                url: Url::parse("http://localhost").unwrap(),
                message: NotificationMessage::default(),
                method: None,
                headers: None,
                secret: None,
                retry_policy: HttpRetryConfig::default(),
            }),
            policy: None,
        }
    }

    /// Sets the Action to use webhook configuration.
    pub fn webhook_config(mut self, url: &str) -> Self {
        self.config = ActionTypeConfig::Webhook(GenericWebhookConfig {
            url: Url::parse(url).unwrap(),
            message: NotificationMessage::default(),
            method: None,
            headers: None,
            secret: None,
            retry_policy: HttpRetryConfig::default(),
        });
        self
    }

    /// Sets the Action to use Slack configuration.
    pub fn slack_config(mut self, url: &str) -> Self {
        self.config = ActionTypeConfig::Slack(SlackConfig {
            slack_url: Url::parse(url).unwrap(),
            message: NotificationMessage::default(),
            retry_policy: HttpRetryConfig::default(),
        });
        self
    }

    /// Sets the Action to use Discord configuration.
    pub fn discord_config(mut self, url: &str) -> Self {
        self.config = ActionTypeConfig::Discord(DiscordConfig {
            discord_url: Url::parse(url).unwrap(),
            message: NotificationMessage::default(),
            retry_policy: HttpRetryConfig::default(),
        });
        self
    }

    /// Sets the Action to use Stdout configuration.
    pub fn stdout_config(mut self, message: Option<NotificationMessage>) -> Self {
        self.config = ActionTypeConfig::Stdout(StdoutConfig { message });
        self
    }

    /// Sets the Action to use Kafka configuration.
    pub fn kafka_config(mut self, brokers: &str, topic: &str) -> Self {
        self.config = ActionTypeConfig::Kafka(KafkaConfig {
            brokers: brokers.to_string(),
            topic: topic.to_string(),
            ..Default::default()
        });
        self
    }

    /// Sets the Action to use RabbitMQ configuration.
    pub fn rabbitmq_config(mut self, uri: &str, exchange: &str, routing_key: &str) -> Self {
        self.config = ActionTypeConfig::RabbitMq(RabbitMqConfig {
            uri: uri.to_string(),
            exchange: exchange.to_string(),
            routing_key: Some(routing_key.to_string()),
            exchange_type: "topic".to_string(),
        });
        self
    }

    /// Sets the Action to use NATS configuration.
    pub fn nats_config(mut self, urls: &str, subject: &str) -> Self {
        self.config = ActionTypeConfig::Nats(NatsConfig {
            urls: urls.to_string(),
            subject: subject.to_string(),
            credentials: None,
        });
        self
    }

    /// Sets the Action policy.
    pub fn policy(mut self, policy: ActionPolicy) -> Self {
        self.policy = Some(policy);
        self
    }

    /// Sets the retry policy for the Action.
    pub fn retry_policy(mut self, retry_policy: HttpRetryConfig) -> Self {
        match &mut self.config {
            ActionTypeConfig::Webhook(cfg) => cfg.retry_policy = retry_policy,
            ActionTypeConfig::Slack(cfg) => cfg.retry_policy = retry_policy,
            ActionTypeConfig::Discord(cfg) => cfg.retry_policy = retry_policy,
            ActionTypeConfig::Telegram(cfg) => cfg.retry_policy = retry_policy,
            _ => { /* No retry policy for other action types */ }
        }
        self
    }

    /// Builds the `ActionConfig` with the provided values.
    pub fn build(self) -> ActionConfig {
        ActionConfig {
            id: self.id,
            name: self.name,
            config: self.config,
            policy: self.policy,
        }
    }
}
