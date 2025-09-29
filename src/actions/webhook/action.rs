use std::sync::Arc;

use reqwest_middleware::ClientWithMiddleware;

use crate::actions::{
    ActionPayload, WebhookComponents, error::ActionDispatcherError, template::TemplateService,
    traits::Action, webhook::WebhookClient,
};

/// An action that sends a webhook notification.
pub struct WebhookAction {
    components: WebhookComponents,
    http_client: Arc<ClientWithMiddleware>,
    template_service: Arc<TemplateService>,
}

impl WebhookAction {
    /// Creates a new `WebhookAction` with the given components, HTTP client,
    /// and template service.
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

        // Determine title and body based on payload type
        let (title, body) = if let ActionPayload::Aggregated { template, .. } = &payload {
            (template.title.clone(), template.body.clone())
        } else {
            (self.components.config.title.clone(), self.components.config.body_template.clone())
        };

        let rendered_title = self.template_service.render(&title, context.clone())?;
        let rendered_body = self.template_service.render(&body, context.clone())?;

        let json_payload = self.components.builder.build_payload(&rendered_title, &rendered_body);
        let client = WebhookClient::new(self.components.config.clone(), self.http_client.clone())?;
        client.notify_json(&json_payload).await?;

        Ok(())
    }
}
