use minijinja::{Environment, context};
use thiserror::Error;

pub struct TemplateService<'a> {
    env: Environment<'a>,
}

#[derive(Debug, Error)]
pub enum TemplateServiceError {
  #[error("Failed to render template")]
  RenderError(#[from] minijinja::Error),
}

impl <'a> TemplateService<'a> {
    pub fn new() -> Self {
        let env = Environment::new();
        // Add templates here
        Self { env }
    }

    pub fn render(&self, template_name: &str, context: serde_json::Value) -> Result<String, TemplateServiceError> {
        let template = self.env.get_template(template_name).unwrap();
        template.render(context).map_err(TemplateServiceError::RenderError)
    }
}
