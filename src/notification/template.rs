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
    /// The environment is configured with a custom missing value callback to
    /// log warnings when template variables are not found in the context.
    pub fn new() -> Self {
        let mut env = Environment::new();
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);

        Self { env }
    }

    /// Renders a template with the given context.
    pub fn render(
        &self,
        template_str: &str,
        context: serde_json::Value,
    ) -> Result<String, TemplateServiceError> {
        tracing::debug!(
            template = template_str,
            context = %context,
            "Rendering template with context."
        );

        match self.env.render_str(template_str, context) {
            Ok(rendered_string) => Ok(rendered_string),
            Err(e) => {
                tracing::warn!("Failed to render template '{}': {}", template_str, e);
                Err(TemplateServiceError::RenderError(e))
            }
        }
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

    #[test]
    fn test_render_template_with_nested_log_params() {
        let service = TemplateService::new();
        let template =
            "From: {{ log.params.from }}, To: {{ log.params.to }}, Value: {{ log.params.value }}";
        let context = json!({
            "log": {
                "contract_address": "0x576e2bed8f7b46d34016198911cdf9886f78bea7",
                "log_index": 105,
                "name": "Transfer",
                "params": {
                    "from": "0xE4b8583cCB95b25737C016ac88E539D0605949e8",
                    "to": "0x035A0C81ceFd37b7c6c638870Ddfa7937C303997",
                    "value": "3141247012536"
                }
            },
            "monitor_name": "All ERC20 Transfers (Ethereum)"
        });
        let result = service.render(template, context).unwrap();
        assert_eq!(
            result,
            "From: 0xE4b8583cCB95b25737C016ac88E539D0605949e8, To: \
             0x035A0C81ceFd37b7c6c638870Ddfa7937C303997, Value: 3141247012536"
        );
    }
}
