use std::sync::Arc;

use argus_core::{
    config::HttpRetryConfig,
    models::action::{DiscordConfig, GenericWebhookConfig, SlackConfig, TelegramConfig},
};
use omnihook::{
    DiscordPayloadBuilder, GenericWebhookPayloadBuilder, SlackPayloadBuilder,
    TelegramPayloadBuilder, WebhookClient, WebhookConfig, WebhookPayloadBuilder,
};
use reqwest_middleware::ClientWithMiddleware;
use url::Url;

use crate::{
    ActionPayload, error::ActionDispatcherError, template::TemplateService, traits::Action,
};

/// Argus-specific glue that maps action model configs to `omnihook` types.
pub struct WebhookComponents {
    pub config: WebhookConfig,
    pub title: String,
    pub body_template: String,
    pub retry_policy: HttpRetryConfig,
    pub builder: PayloadBuilder,
}

/// An enum representing the different types of payload builders supported by
/// Argus.
pub enum PayloadBuilder {
    Slack(SlackPayloadBuilder),
    Discord(DiscordPayloadBuilder),
    Telegram(TelegramPayloadBuilder),
    Generic(GenericWebhookPayloadBuilder),
}

impl WebhookPayloadBuilder for PayloadBuilder {
    fn build_payload(&self, title: &str, body: &str) -> serde_json::Value {
        match self {
            PayloadBuilder::Slack(b) => b.build_payload(title, body),
            PayloadBuilder::Discord(b) => b.build_payload(title, body),
            PayloadBuilder::Telegram(b) => b.build_payload(title, body),
            PayloadBuilder::Generic(b) => b.build_payload(title, body),
        }
    }
}

impl From<&GenericWebhookConfig> for WebhookComponents {
    fn from(c: &GenericWebhookConfig) -> Self {
        // Build the WebhookConfig with optional method and headers
        let config = c
            .method
            .as_deref()
            .and_then(|m| m.parse().ok())
            .into_iter()
            .fold(WebhookConfig::new(c.url.clone()), |cfg, m| cfg.with_method(m))
            .with_headers(c.headers.as_ref().into_iter().flatten());

        let config = c.secret.as_deref().into_iter().fold(config, |cfg, s| cfg.with_secret(s));

        WebhookComponents {
            config,
            title: c.message.title.clone(),
            body_template: c.message.body.clone(),
            retry_policy: c.retry_policy.clone(),
            builder: PayloadBuilder::Generic(GenericWebhookPayloadBuilder),
        }
    }
}

impl From<&DiscordConfig> for WebhookComponents {
    fn from(c: &DiscordConfig) -> Self {
        WebhookComponents {
            config: WebhookConfig::new(c.discord_url.clone()),
            title: c.message.title.clone(),
            body_template: c.message.body.clone(),
            retry_policy: c.retry_policy.clone(),
            builder: PayloadBuilder::Discord(DiscordPayloadBuilder),
        }
    }
}

impl From<&TelegramConfig> for WebhookComponents {
    fn from(c: &TelegramConfig) -> Self {
        let url =
            Url::parse(&format!("https://api.telegram.org/bot{}/sendMessage", c.token)).unwrap();
        WebhookComponents {
            config: WebhookConfig::new(url),
            title: c.message.title.clone(),
            body_template: c.message.body.clone(),
            retry_policy: c.retry_policy.clone(),
            builder: PayloadBuilder::Telegram(TelegramPayloadBuilder {
                chat_id: c.chat_id.clone(),
                disable_web_preview: c.disable_web_preview.unwrap_or(false),
            }),
        }
    }
}

impl From<&SlackConfig> for WebhookComponents {
    fn from(c: &SlackConfig) -> Self {
        WebhookComponents {
            config: WebhookConfig::new(c.slack_url.clone()),
            title: c.message.title.clone(),
            body_template: c.message.body.clone(),
            retry_policy: c.retry_policy.clone(),
            builder: PayloadBuilder::Slack(SlackPayloadBuilder),
        }
    }
}

/// An action that sends a webhook notification via `omnihook`.
pub struct WebhookAction {
    client: WebhookClient,
    title: String,
    body_template: String,
    builder: PayloadBuilder,
    template_service: Arc<TemplateService>,
}

impl WebhookAction {
    pub fn try_new(
        components: WebhookComponents,
        http_client: Arc<ClientWithMiddleware>,
        template_service: Arc<TemplateService>,
    ) -> Result<Self, ActionDispatcherError> {
        let client = WebhookClient::new(components.config, http_client)?;
        Ok(Self {
            client,
            title: components.title,
            body_template: components.body_template,
            builder: components.builder,
            template_service,
        })
    }
}

#[async_trait::async_trait]
impl Action for WebhookAction {
    async fn execute(&self, payload: ActionPayload) -> Result<(), ActionDispatcherError> {
        let (title, body) = match &payload {
            ActionPayload::Aggregated { template, .. } => {
                (template.title.clone(), template.body.clone())
            }
            _ => (self.title.clone(), self.body_template.clone()),
        };

        let context = payload.context()?;

        let rendered_title = self.template_service.render(&title, context.clone())?;
        let rendered_body = self.template_service.render(&body, context)?;

        let idempotency_key = payload.idempotency_key();

        self.client
            .notify_with_key(
                &rendered_title,
                &rendered_body,
                &self.builder,
                Some(idempotency_key.as_str()),
            )
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use argus_core::models::notification::NotificationMessage;

    use super::*;

    #[test]
    fn test_payload_builder_dispatch() {
        let title = "Test Title";
        let body = "Test Body";

        let slack = PayloadBuilder::Slack(SlackPayloadBuilder);
        let slack_payload = slack.build_payload(title, body);
        assert!(slack_payload.get("blocks").is_some());

        let discord = PayloadBuilder::Discord(DiscordPayloadBuilder);
        let discord_payload = discord.build_payload(title, body);
        assert_eq!(discord_payload["content"], "Test Title\n\nTest Body");

        let telegram = PayloadBuilder::Telegram(TelegramPayloadBuilder {
            chat_id: "123".into(),
            disable_web_preview: true,
        });
        let telegram_payload = telegram.build_payload(title, body);
        assert_eq!(telegram_payload["chat_id"], "123");
        assert_eq!(telegram_payload["disable_web_page_preview"], true);

        let generic = PayloadBuilder::Generic(GenericWebhookPayloadBuilder);
        let generic_payload = generic.build_payload(title, body);
        assert_eq!(generic_payload["title"], title);
        assert_eq!(generic_payload["body"], body);
    }

    #[test]
    fn test_webhook_components_from_generic_full() {
        let url = Url::parse("https://example.com").unwrap();
        let mut headers = std::collections::HashMap::new();
        headers.insert("X-Custom".to_string(), "Value".to_string());

        let config = GenericWebhookConfig {
            url: url.clone(),
            message: NotificationMessage { title: "T".into(), body: "B".into() },
            method: Some("PATCH".to_string()),
            secret: Some("secret123".to_string()),
            headers: Some(headers),
            retry_policy: HttpRetryConfig::default(),
        };

        let components = WebhookComponents::from(&config);
        assert_eq!(components.title, "T");
        assert_eq!(components.body_template, "B");
    }

    #[test]
    fn test_webhook_components_from_telegram_with_preview() {
        let config = TelegramConfig {
            token: "bot123".into(),
            chat_id: "987".into(),
            message: NotificationMessage { title: "T".into(), body: "B".into() },
            disable_web_preview: Some(true),
            retry_policy: HttpRetryConfig::default(),
        };

        let components = WebhookComponents::from(&config);
        if let PayloadBuilder::Telegram(ref b) = components.builder {
            assert_eq!(b.chat_id, "987");
            assert!(b.disable_web_preview);
        } else {
            panic!("Expected Telegram builder");
        }
    }
}
