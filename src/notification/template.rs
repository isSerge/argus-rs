//! This module provides a service for rendering templates using the minijinja
//! templating engine.

use minijinja::Environment;
use thiserror::Error;

/// A service for rendering templates using the minijinja templating engine.
pub struct TemplateService {
    env: Environment<'static>,
}

/// Error type for the TemplateService.
#[derive(Debug, Error)]
pub enum TemplateServiceError {
    #[error("Failed to render template")]
    RenderError(#[from] minijinja::Error),
}

impl TemplateService {
    /// Creates a new instance of `TemplateService` with a default environment.
    pub fn new() -> Self {
        let env = Environment::new();
        // Add templates here
        Self { env }
    }

    /// Renders a template with the given context.
    pub fn render(
        &self,
        template_str: &str,
        context: serde_json::Value,
    ) -> Result<String, TemplateServiceError> {
        self.env.render_str(template_str, context).map_err(TemplateServiceError::RenderError)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_render_template_with_context() {
        let service = TemplateService::new();
        let template = "Block number: {{ block.number }}, Tx hash: {{ transaction.hash }}";
        let context = json!({
            "block": {
                "number": 123
            },
            "transaction": {
                "hash": "0xabc"
            }
        });
        let result = service.render(template, context).unwrap();
        assert_eq!(result, "Block number: 123, Tx hash: 0xabc");
    }

    #[test]
    fn test_render_template_with_invalid_template() {
        let service = TemplateService::new();
        let template = "Hello, {{ name }";
        let context = json!({ "name": "World" });
        let result = service.render(template, context);
        assert!(result.is_err());
        assert!(matches!(result, Err(TemplateServiceError::RenderError(_))));
    }
}
