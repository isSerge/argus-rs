use url::Url;

use super::*;
use crate::{
    config::HttpRetryConfig,
    models::action::{DiscordConfig, GenericWebhookConfig, SlackConfig, TelegramConfig},
};

/// A private container struct holding the generic components required to send
/// any webhook-based notification.
///
/// This struct provides a common interface for the `ActionDispatcher` to
/// work with, regardless of the underlying provider (e.g., Slack, Discord).
pub struct WebhookComponents {
    /// The generic webhook configuration, including the URL, method, and
    /// headers.
    pub config: WebhookConfig,
    /// The specific retry policy for this notification channel.
    pub retry_policy: HttpRetryConfig,
    /// The payload builder responsible for creating the channel-specific JSON
    /// body.
    pub builder: Box<dyn WebhookPayloadBuilder>,
}

impl From<&GenericWebhookConfig> for WebhookComponents {
    fn from(c: &GenericWebhookConfig) -> Self {
        WebhookComponents {
            config: WebhookConfig {
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
            config: WebhookConfig {
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
            config: WebhookConfig {
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
            config: WebhookConfig {
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
