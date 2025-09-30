use std::sync::Arc;

use crate::{
    actions::{
        ActionPayload, error::ActionDispatcherError, template::TemplateService, traits::Action,
    },
    models::action::StdoutConfig,
};

/// An action that prints a message to standard output.
pub struct StdoutAction {
    config: StdoutConfig,
    template_service: Arc<TemplateService>,
}

impl StdoutAction {
    /// Creates a new `StdoutAction` with the given configuration and template
    /// service.
    pub fn new(config: StdoutConfig, template_service: Arc<TemplateService>) -> Self {
        Self { config, template_service }
    }
}

#[async_trait::async_trait]
impl Action for StdoutAction {
    async fn execute(&self, payload: ActionPayload) -> Result<(), ActionDispatcherError> {
        let context = payload.context()?;

        if let Some(message) = &self.config.message {
            let rendered_title = self.template_service.render(&message.title, context.clone())?;
            let rendered_body = self.template_service.render(&message.body, context.clone())?;
            println!(
                "=== Stdout Action: {} ===\n{}\n{}\n",
                payload.action_name(),
                rendered_title,
                rendered_body
            );
        } else {
            println!("=== Stdout Action: {} ===\n {}\n", payload.action_name(), context);
        }
        Ok(())
    }
}
