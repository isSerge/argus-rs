use url::Url;

use crate::{
    config::HttpRetryConfig,
    models::{
        notification::NotificationMessage,
        notifier::{
            DiscordConfig, NotifierConfig, NotifierPolicy, NotifierTypeConfig, SlackConfig,
            WebhookConfig,
        },
    },
};

/// A builder for creating `NotifierConfig` instances for testing.
pub struct NotifierBuilder {
    name: String,
    config: NotifierTypeConfig,
    policy: Option<NotifierPolicy>,
}

impl NotifierBuilder {
    /// Creates a new `NotifierBuilder` with the given name.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            config: NotifierTypeConfig::Webhook(WebhookConfig {
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

    /// Sets the notifier to use webhook configuration.
    pub fn webhook_config(mut self, url: &str) -> Self {
        self.config = NotifierTypeConfig::Webhook(WebhookConfig {
            url: Url::parse(url).unwrap(),
            message: NotificationMessage::default(),
            method: None,
            headers: None,
            secret: None,
            retry_policy: HttpRetryConfig::default(),
        });
        self
    }

    /// Sets the notifier to use Slack configuration.
    pub fn slack_config(mut self, url: &str) -> Self {
        self.config = NotifierTypeConfig::Slack(SlackConfig {
            slack_url: Url::parse(url).unwrap(),
            message: NotificationMessage::default(),
            retry_policy: HttpRetryConfig::default(),
        });
        self
    }

    /// Sets the notifier to use Discord configuration.
    pub fn discord_config(mut self, url: &str) -> Self {
        self.config = NotifierTypeConfig::Discord(DiscordConfig {
            discord_url: Url::parse(url).unwrap(),
            message: NotificationMessage::default(),
            retry_policy: HttpRetryConfig::default(),
        });
        self
    }

    /// Sets the notifier policy.
    pub fn policy(mut self, policy: NotifierPolicy) -> Self {
        self.policy = Some(policy);
        self
    }

    /// Sets the retry policy for the notifier.
    pub fn retry_policy(mut self, retry_policy: HttpRetryConfig) -> Self {
        match &mut self.config {
            NotifierTypeConfig::Webhook(cfg) => cfg.retry_policy = retry_policy,
            NotifierTypeConfig::Slack(cfg) => cfg.retry_policy = retry_policy,
            NotifierTypeConfig::Discord(cfg) => cfg.retry_policy = retry_policy,
            NotifierTypeConfig::Telegram(cfg) => cfg.retry_policy = retry_policy,
        }
        self
    }

    /// Builds the `NotifierConfig` with the provided values.
    pub fn build(self) -> NotifierConfig {
        NotifierConfig { name: self.name, config: self.config, policy: self.policy }
    }
}
