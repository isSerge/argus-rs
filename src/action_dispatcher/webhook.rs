use std::sync::Arc;

use omnihook::{
    DiscordPayloadBuilder, GenericWebhookPayloadBuilder, SlackPayloadBuilder,
    TelegramPayloadBuilder, WebhookClient, WebhookConfig, WebhookPayloadBuilder,
};
use reqwest_middleware::ClientWithMiddleware;
use url::Url;

use crate::{
    action_dispatcher::{
        ActionPayload, error::ActionDispatcherError, template::TemplateService, traits::Action,
    },
    config::HttpRetryConfig,
    models::action::{DiscordConfig, GenericWebhookConfig, SlackConfig, TelegramConfig},
};

/// Argus-specific glue that maps action model configs to `omnihook` types.
pub struct WebhookComponents {
    pub config: WebhookConfig,
    pub retry_policy: HttpRetryConfig,
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

/// An action that sends a webhook notification via `omnihook`.
pub struct WebhookAction {
    components: WebhookComponents,
    http_client: Arc<ClientWithMiddleware>,
    template_service: Arc<TemplateService>,
}

impl WebhookAction {
    pub fn new(
        components: WebhookComponents,
        http_client: Arc<ClientWithMiddleware>,
        template_service: Arc<TemplateService>,
    ) -> Self {
        Self { components, http_client, template_service }
    }
}

#[async_trait::async_trait]
impl Action for WebhookAction {
    async fn execute(&self, payload: ActionPayload) -> Result<(), ActionDispatcherError> {
        let context = payload.context()?;
        let idempotency_key = payload.idempotency_key();

        let (title, body) = if let ActionPayload::Aggregated { template, .. } = &payload {
            (template.title.clone(), template.body.clone())
        } else {
            (self.components.config.title.clone(), self.components.config.body_template.clone())
        };

        let rendered_title = self.template_service.render(&title, context.clone())?;
        let rendered_body = self.template_service.render(&body, context.clone())?;

        let json_payload = self.components.builder.build_payload(&rendered_title, &rendered_body);
        let client = WebhookClient::new(self.components.config.clone(), self.http_client.clone())?;
        client.notify_json(&json_payload, Some(&idempotency_key)).await?;

        Ok(())
    }
}
