//! This module provides a service for rendering templates using the minijinja
//! templating engine.

pub mod filters;

use std::collections::HashSet;

use minijinja::Environment;
use thiserror::Error;

/// A service for rendering templates using the minijinja templating engine.
pub struct TemplateService {
    env: Environment<'static>,
}

/// Error type for the TemplateService.
#[derive(Debug, Error)]
pub enum TemplateServiceError {
    /// An error occurred while rendering the template.
    #[error("Failed to render template")]
    RenderError(#[from] minijinja::Error),
}

impl Default for TemplateService {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateService {
    /// Creates a new instance of `TemplateService` with a default environment.
    /// The environment is configured with a custom missing value callback to
    /// log warnings when template variables are not found in the context.
    pub fn new() -> Self {
        let mut env = Environment::new();
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);

        env.add_filter("sum", filters::sum);
        env.add_filter("avg", filters::avg);
        env.add_filter("decimals", filters::decimals);
        env.add_filter("ether", filters::ether);
        env.add_filter("gwei", filters::gwei);
        env.add_filter("usdc", filters::usdc);
        env.add_filter("usdt", filters::usdt);
        env.add_filter("wbtc", filters::wbtc);

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

    /// Extracts all unique variable keys from a template string.
    pub fn extract_variables(
        &self,
        template_str: &str,
    ) -> Result<HashSet<String>, TemplateServiceError> {
        let tmpl = self.env.template_from_str(template_str)?;
        Ok(tmpl.undeclared_variables(true))
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

    #[test]
    fn test_sum_and_format_with_ether_filter() {
        let service = TemplateService::new();
        let template = "{{ matches | map(attribute='tx.value') | sum | ether }}";
        let context = json!({
            "matches": [
                {"tx": {"value": "1000000000000000000"}}, // 1 ETH
                {"tx": {"value": "500000000000000000"}}   // 0.5 ETH
            ]
        });
        let result = service.render(template, context).unwrap();
        assert_eq!(result, "1.5");
    }

    #[test]
    fn test_avg_and_format_with_gwei_filter() {
        let service = TemplateService::new();
        let template = "{{ matches | map(attribute='tx.gas_price') | avg | gwei }}";
        let context = json!({
            "matches": [
                {"tx": {"gas_price": "20000000000"}}, // 20 Gwei
                {"tx": {"gas_price": "40000000000"}}  // 40 Gwei
            ]
        });
        let result = service.render(template, context).unwrap();
        assert_eq!(result, "30");
    }

    #[test]
    fn test_template_service_default_implementation() {
        let service = TemplateService::default();
        let template = "Hello, {{ name }}!";
        let context = json!({ "name": "World" });
        let result = service.render(template, context).unwrap();
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_extract_variables() {
        let service = TemplateService::new();
        let template = "Block number: {{ block.number }}, Tx hash: {{ transaction.hash }}";
        let variables = service.extract_variables(template).unwrap();
        let expected: HashSet<_> =
            ["block.number", "transaction.hash"].iter().map(|s| s.to_string()).collect();
        assert_eq!(variables, expected);
    }
}
