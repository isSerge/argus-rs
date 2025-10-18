//! Module for loading and validating monitor configurations.

use std::sync::Arc;

use alloy::{json_abi::JsonAbi, primitives::Address};
use serde_json::{Value, json};
use thiserror::Error;

use crate::{
    abi::AbiService,
    actions::template::{TemplateService, TemplateServiceError},
    engine::rhai::{RhaiScriptValidationError, RhaiScriptValidationResult, RhaiScriptValidator},
    models::{action::ActionConfig, monitor::MonitorConfig},
};

/// Validates monitor addresses and determines monitor type.
struct AddressValidator;

/// Validates action templates using a dummy context.
struct TemplateValidator {
    template_service: Arc<TemplateService>,
}

/// Validates that actions exist and monitors reference valid actions.
struct ActionValidator<'a> {
    actions: &'a [ActionConfig],
}

/// Validates calldata configuration rules.
struct CalldataValidator;

/// A coordinator that orchestrates validation of monitor configurations using
/// specialized validators.
pub struct MonitorValidator<'a> {
    /// The application network ID.
    network_id: &'a str,

    /// Validates addresses and determines monitor types.
    address_validator: AddressValidator,

    /// Validates action templates.
    template_validator: TemplateValidator,

    /// Validates action references.
    action_validator: ActionValidator<'a>,

    /// Validates calldata configuration.
    calldata_validator: CalldataValidator,

    /// The script validator for validating Rhai scripts.
    script_validator: RhaiScriptValidator,

    /// The ABI service for retrieving contract ABIs.
    abi_service: Arc<AbiService>,
}

/// An error that occurs during monitor validation.
#[derive(Debug, Error)]
pub enum MonitorValidationError {
    /// A monitor that accesses log data does not have a contract address
    /// specified, and is not configured as a global log monitor.
    #[error(
        "Monitor '{monitor_name}' accesses log data ('log.*') but is not tied to a specific \
         contract address, nor is it configured as a global log monitor ('address: all'). Please \
         provide an 'address' or set 'address: all' for this monitor."
    )]
    MonitorRequiresAddress {
        /// The name of the monitor that failed validation.
        monitor_name: String,
    },

    /// A monitor that accesses log data does not have an ABI defined.
    #[error(
        "Monitor '{monitor_name}' accesses log data but does not have an ABI defined. {reason}"
    )]
    MonitorRequiresAbi {
        /// The name of the monitor that failed validation.
        monitor_name: String,
        /// The reason why the ABI is required but not found/linked.
        reason: String,
    },

    /// The address provided for a monitor is invalid.
    #[error("Invalid address for monitor '{monitor_name}': {address}")]
    InvalidAddress {
        /// The name of the monitor.
        monitor_name: String,
        /// The invalid address.
        address: String,
    },

    /// The monitor is configured for a different network than expected.
    #[error(
        "Monitor '{monitor_name}' is configured for network '{expected_network}', but it is \
         actually on network '{actual_network}'."
    )]
    InvalidNetwork {
        /// The name of the monitor that failed validation.
        monitor_name: String,
        /// The expected network ID.
        expected_network: String,
        /// The actual network ID of the monitor.
        actual_network: String,
    },

    /// The monitor references a action that does not exist.
    #[error("Monitor '{monitor_name}' references an unknown action: '{action_name}'")]
    UnknownAction {
        /// The name of the monitor.
        monitor_name: String,
        /// The name of the unknown action.
        action_name: String,
    },

    /// An error occurred during script validation.
    #[error("Script validation error for monitor '{monitor_name}': {error}")]
    ScriptError {
        /// The name of the monitor that failed validation.
        monitor_name: String,
        /// The error that occurred during script validation.
        error: RhaiScriptValidationError,
    },

    /// A monitor is configured to decode calldata but is missing a required
    /// field.
    #[error("Invalid configuration for calldata decoding on monitor '{monitor_name}': {reason}")]
    InvalidCalldataConfig {
        /// The name of the monitor.
        monitor_name: String,
        /// The reason for the validation failure.
        reason: String,
    },

    /// An action's template references a field that is not available for the
    /// monitor.
    #[error(
        "Action '{action_name}' on monitor '{monitor_name}' has an invalid template: \
         '{template}'. The variable '{invalid_variable}' is not available for this monitor."
    )]
    InvalidActionTemplate {
        /// The name of the monitor.
        monitor_name: String,
        /// The name of the action.
        action_name: String,
        /// The invalid template.
        template: String,
        /// The invalid variable.
        invalid_variable: String,
    },
}

impl<'a> MonitorValidator<'a> {
    /// Creates a new `MonitorValidator` for the specified network ID.
    pub fn new(
        script_validator: RhaiScriptValidator,
        abi_service: Arc<AbiService>,
        template_service: Arc<TemplateService>,
        network_id: &'a str,
        actions: &'a [ActionConfig],
    ) -> Self {
        MonitorValidator {
            network_id,
            address_validator: AddressValidator,
            template_validator: TemplateValidator { template_service },
            action_validator: ActionValidator { actions },
            calldata_validator: CalldataValidator,
            script_validator,
            abi_service,
        }
    }

    /// Validates the given monitor configuration.
    pub fn validate(&self, monitor: &MonitorConfig) -> Result<(), MonitorValidationError> {
        tracing::debug!(monitor = ?monitor, "Validating monitor configuration...");

        // Validate network compatibility
        self.validate_network(monitor)?;

        // Validate action references
        self.action_validator.validate(monitor)?;

        // Parse and validate address
        let (parsed_address, is_global_log_monitor) =
            self.address_validator.parse_and_validate(monitor)?;

        // Retrieve ABI if available
        let abi_json = self.get_monitor_abi_json(monitor, parsed_address);

        // Validate the script
        let script_validation_result = self.validate_script(monitor, abi_json.as_deref())?;

        // Validate script and ABI compatibility rules
        self.validate_script_and_abi_rules(
            monitor,
            &script_validation_result,
            is_global_log_monitor,
            abi_json.is_some(),
        )?;

        // Validate calldata configuration if needed
        if script_validation_result.ast_analysis.accesses_call_variable {
            self.calldata_validator.validate(monitor)?;
        }

        // Validate action templates
        self.template_validator.validate(
            monitor,
            &self.action_validator,
            abi_json.as_deref(),
            &script_validation_result,
        )?;

        Ok(())
    }

    /// Validates that the monitor is configured for the correct network.
    fn validate_network(&self, monitor: &MonitorConfig) -> Result<(), MonitorValidationError> {
        if monitor.network != self.network_id {
            return Err(MonitorValidationError::InvalidNetwork {
                monitor_name: monitor.name.clone(),
                expected_network: self.network_id.to_string(),
                actual_network: monitor.network.clone(),
            });
        }
        Ok(())
    }

    /// Validates the monitor's script.
    fn validate_script(
        &self,
        monitor: &MonitorConfig,
        abi: Option<&JsonAbi>,
    ) -> Result<RhaiScriptValidationResult, MonitorValidationError> {
        self.script_validator.validate_script(&monitor.filter_script, abi).map_err(|e| {
            MonitorValidationError::ScriptError { monitor_name: monitor.name.clone(), error: e }
        })
    }

    /// Gets the ABI JSON for the monitor based on its configuration.
    /// Returns Some if an ABI name is provided, or if a contract address is
    /// provided and a cached contract exists for that address.
    fn get_monitor_abi_json(
        &self,
        monitor: &MonitorConfig,
        parsed_address: Option<Address>,
    ) -> Option<Arc<JsonAbi>> {
        if let Some(abi_name) = &monitor.abi {
            // If an ABI name is provided, always use get_abi_by_name
            // This works for both contract-specific and global monitors
            self.abi_service.get_abi_by_name(abi_name)
        } else if let Some(address) = parsed_address {
            // For contract-specific monitors without explicit ABI name,
            // try to get a cached contract by address
            self.abi_service.get_abi(address).map(|c| c.abi.clone())
        } else {
            None // No ABI name provided and no cached contract available
        }
    }

    /// Enforces address and ABI requirements based on script analysis.
    fn validate_script_and_abi_rules(
        &self,
        monitor: &MonitorConfig,
        script_result: &RhaiScriptValidationResult,
        is_global_log_monitor: bool,
        abi_was_retrieved: bool,
    ) -> Result<(), MonitorValidationError> {
        if !script_result.ast_analysis.accesses_log_variable {
            return Ok(()); // No log access, no special rules apply.
        }

        if is_global_log_monitor {
            // Rules for global log monitors (`address: all`)
            if monitor.abi.is_none() {
                return Err(MonitorValidationError::MonitorRequiresAbi {
                    monitor_name: monitor.name.clone(),
                    reason: "ABI is required for global log monitoring (address: all) to decode \
                             logs."
                        .to_string(),
                });
            }
            if !abi_was_retrieved {
                return Err(MonitorValidationError::MonitorRequiresAbi {
                    monitor_name: monitor.name.clone(),
                    reason: format!(
                        "ABI '{}' could not be retrieved for global log monitor. Ensure the ABI \
                         is loaded.",
                        monitor.abi.as_ref().unwrap()
                    ),
                });
            }
        } else {
            // Rules for specific-address log monitors
            if monitor.address.is_none() {
                return Err(MonitorValidationError::MonitorRequiresAddress {
                    monitor_name: monitor.name.clone(),
                });
            }
            if monitor.abi.is_none() {
                return Err(MonitorValidationError::MonitorRequiresAbi {
                    monitor_name: monitor.name.clone(),
                    reason: "ABI is required for contract-specific log monitoring to decode logs."
                        .to_string(),
                });
            }
            if !abi_was_retrieved {
                return Err(MonitorValidationError::MonitorRequiresAbi {
                    monitor_name: monitor.name.clone(),
                    reason: format!(
                        "ABI '{}' could not be retrieved for address '{}'. Ensure the ABI is \
                         loaded and linked.",
                        monitor.abi.as_ref().unwrap(),
                        monitor.address.as_ref().unwrap() /* Safe unwrap as we checked for None
                                                           * above */
                    ),
                });
            }
        }
        Ok(())
    }
}

impl AddressValidator {
    /// Parses and validates the address for the monitor.
    /// Returns the parsed address (if any) and a flag indicating if it's a
    /// global log monitor.
    fn parse_and_validate(
        &self,
        monitor: &MonitorConfig,
    ) -> Result<(Option<Address>, bool), MonitorValidationError> {
        match &monitor.address {
            None => Ok((None, false)),
            Some(address_str) if address_str.eq_ignore_ascii_case("all") => Ok((None, true)),
            Some(address_str) => {
                let parsed_address = address_str.parse::<Address>().map_err(|_| {
                    MonitorValidationError::InvalidAddress {
                        monitor_name: monitor.name.clone(),
                        address: address_str.clone(),
                    }
                })?;
                Ok((Some(parsed_address), false))
            }
        }
    }
}

impl<'a> ActionValidator<'a> {
    /// Validates that all actions referenced by the monitor exist.
    fn validate(&self, monitor: &MonitorConfig) -> Result<(), MonitorValidationError> {
        if monitor.actions.is_empty() {
            tracing::warn!(
                monitor_name = monitor.name,
                "Monitor has no actions configured and will not send any notifications."
            );
            return Ok(());
        }

        // Check if all actions exist
        for action_name in &monitor.actions {
            if !self.action_exists(action_name) {
                return Err(MonitorValidationError::UnknownAction {
                    monitor_name: monitor.name.clone(),
                    action_name: action_name.clone(),
                });
            }
        }

        Ok(())
    }

    /// Checks if an action with the given name exists.
    fn action_exists(&self, action_name: &str) -> bool {
        self.actions.iter().any(|action| action.name == action_name)
    }

    /// Finds an action by name.
    fn find_action(&self, action_name: &str) -> Option<&ActionConfig> {
        self.actions.iter().find(|action| action.name == action_name)
    }
}

impl CalldataValidator {
    /// Validates the calldata decoding rules for the monitor.
    fn validate(&self, monitor: &MonitorConfig) -> Result<(), MonitorValidationError> {
        // Check if ABI is provided when calldata decoding is enabled.
        if monitor.abi.is_none() {
            return Err(MonitorValidationError::InvalidCalldataConfig {
                monitor_name: monitor.name.clone(),
                reason: "Calldata decoding is enabled, but no ABI is specified. Please provide an \
                         ABI name."
                    .to_string(),
            });
        }

        // Check if address is provided when calldata decoding is enabled.
        match monitor.address.as_deref() {
            // Global address case
            Some(addr) if addr.eq_ignore_ascii_case("all") =>
                Err(MonitorValidationError::InvalidCalldataConfig {
                    monitor_name: monitor.name.clone(),
                    reason: "Calldata decoding cannot be enabled for global monitors (address: \
                             all). Please specify a concrete contract address."
                        .to_string(),
                }),
            // Specific address case
            Some(_) => Ok(()),
            // No address case
            None => Err(MonitorValidationError::InvalidCalldataConfig {
                monitor_name: monitor.name.clone(),
                reason: "Calldata decoding is enabled, but no contract address is specified. \
                         Please provide a contract address."
                    .to_string(),
            }),
        }
    }
}

impl TemplateValidator {
    /// Validates the templates of all actions associated with the monitor.
    fn validate(
        &self,
        monitor: &MonitorConfig,
        action_validator: &ActionValidator,
        abi: Option<&JsonAbi>,
        script_result: &RhaiScriptValidationResult,
    ) -> Result<(), MonitorValidationError> {
        let context = Self::generate_dummy_context(monitor, abi, script_result);

        for action_name in &monitor.actions {
            self.validate_single_action_template(monitor, action_name, action_validator, &context)?;
        }

        Ok(())
    }

    /// Validates the template of a single action.
    fn validate_single_action_template(
        &self,
        monitor: &MonitorConfig,
        action_name: &str,
        action_validator: &ActionValidator,
        context: &serde_json::Value,
    ) -> Result<(), MonitorValidationError> {
        // Find the action (this should always succeed due to earlier validation)
        let action = action_validator
            .find_action(action_name)
            .expect("Action should exist due to earlier validation");

        // Serialize the action to JSON and extract all template strings
        let action_value = serde_json::to_value(action).unwrap_or(Value::Null);
        let templates = Self::extract_templates(&action_value);

        // Validate each template by attempting to render it
        for template in templates {
            if let Err(e) = self.template_service.render(&template, context.clone()) {
                let invalid_variable = match e {
                    TemplateServiceError::RenderError(e) => e.to_string(),
                };
                return Err(MonitorValidationError::InvalidActionTemplate {
                    monitor_name: monitor.name.clone(),
                    action_name: action_name.to_string(),
                    template,
                    invalid_variable,
                });
            }
        }

        Ok(())
    }

    /// Recursively extracts template strings from a JSON value.
    /// A template string is one that contains both "{{" and "}}" and appears to
    /// be a valid template.
    fn extract_templates(val: &Value) -> Vec<String> {
        let mut templates = Vec::new();
        Self::find_templates_recursive(val, &mut templates);
        templates
    }

    /// Helper function to recursively find template strings in JSON values.
    fn find_templates_recursive(val: &Value, templates: &mut Vec<String>) {
        match val {
            Value::String(s) =>
                if Self::is_template_string(s) {
                    templates.push(s.clone());
                },
            Value::Object(obj) =>
                for v in obj.values() {
                    Self::find_templates_recursive(v, templates);
                },
            Value::Array(arr) =>
                for v in arr {
                    Self::find_templates_recursive(v, templates);
                },
            _ => {}
        }
    }

    /// Checks if a string appears to be a template string.
    /// This is a simple heuristic that looks for "{{" and "}}" patterns.
    fn is_template_string(s: &str) -> bool {
        s.contains("{{") && s.contains("}}")
    }

    /// Generates a dummy context for validating action templates.
    /// This context includes all possible variables that could be available to
    /// a template, allowing for a "dry run" rendering to catch invalid
    /// field access.
    fn generate_dummy_context(
        _monitor: &MonitorConfig,
        abi: Option<&JsonAbi>,
        script_result: &RhaiScriptValidationResult,
    ) -> serde_json::Value {
        let mut context = serde_json::Map::new();

        // Always include transaction data as it's the most basic context
        context.insert("transaction".to_string(), Self::create_dummy_transaction());

        // Add log context if the script accesses log variables
        if script_result.ast_analysis.accesses_log_variable {
            context.insert("log".to_string(), Self::create_dummy_log_context(abi));
        }

        // Add calldata context if the script accesses call variables
        if script_result.ast_analysis.accesses_call_variable {
            context.insert("decoded_call".to_string(), Self::create_dummy_calldata_context(abi));
        }

        serde_json::Value::Object(context)
    }

    /// Creates a dummy transaction object with all expected fields.
    fn create_dummy_transaction() -> serde_json::Value {
        json!({
            "hash": "0x000...",
            "index": 0,
            "from": "0x000...",
            "to": "0x000...",
            "value": "0",
            "input": "0x000...",
            "nonce": 0,
            "gas_limit": "0",
            "gas_price": "0",
            "max_fee_per_gas": "0",
            "max_priority_fee_per_gas": "0",
            "v": "0",
            "r": "0x000...",
            "s": "0x000...",
        })
    }

    /// Creates a dummy log context with parameters from the ABI events.
    fn create_dummy_log_context(abi: Option<&JsonAbi>) -> serde_json::Value {
        let params = match abi {
            Some(abi) => Self::extract_event_parameters(abi),
            None => serde_json::Map::new(),
        };

        json!({
            "name": "DummyEvent",
            "params": params,
        })
    }

    /// Creates a dummy calldata context with inputs from the ABI functions.
    fn create_dummy_calldata_context(abi: Option<&JsonAbi>) -> serde_json::Value {
        let inputs = match abi {
            Some(abi) => Self::extract_function_inputs(abi),
            None => serde_json::Map::new(),
        };

        json!({
            "name": "DummyFunction",
            "inputs": inputs,
        })
    }

    /// Extracts all unique parameter names from events in the ABI.
    fn extract_event_parameters(abi: &JsonAbi) -> serde_json::Map<String, serde_json::Value> {
        let mut params = serde_json::Map::new();
        for event in abi.events.values().flatten() {
            for input in &event.inputs {
                params.entry(input.name.clone()).or_insert_with(|| json!(""));
            }
        }
        params
    }

    /// Extracts all unique input parameter names from functions in the ABI.
    fn extract_function_inputs(abi: &JsonAbi) -> serde_json::Map<String, serde_json::Value> {
        let mut inputs = serde_json::Map::new();
        for function in abi.functions.values().flatten() {
            for input in &function.inputs {
                inputs.entry(input.name.clone()).or_insert_with(|| json!(""));
            }
        }
        inputs
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use alloy::primitives::{Address, address};
    use tempfile::tempdir;

    use crate::{
        abi::{AbiService, repository::AbiRepository},
        actions::template::TemplateService,
        config::RhaiConfig,
        engine::rhai::{RhaiCompiler, RhaiScriptValidationError, RhaiScriptValidator},
        models::{action::ActionConfig, monitor::MonitorConfig},
        monitor::{MonitorValidationError, MonitorValidator},
        test_helpers::{ActionBuilder, erc20_abi_json},
    };

    fn create_test_monitor(
        id: i64,
        address: Option<&str>,
        abi: Option<&str>,
        script: &str,
        actions: Vec<String>,
    ) -> MonitorConfig {
        MonitorConfig::from_config(
            format!("Test Monitor {id}"),
            "testnet".to_string(),
            address.map(String::from),
            abi.map(String::from),
            script.to_string(),
            actions,
        )
    }

    fn create_test_action(name: &str) -> ActionConfig {
        ActionBuilder::new(name).discord_config("https://discord.com/api/webhooks/test").build()
    }

    fn create_monitor_validator<'a>(
        actions: &'a [ActionConfig],
        abi_to_preload: Option<(Address, &'static str, &'static str)>,
    ) -> MonitorValidator<'a> {
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config));
        let script_validator = RhaiScriptValidator::new(compiler);
        let template_service = Arc::new(TemplateService::new());

        let temp_dir = tempdir().unwrap();
        let abi_dir_path = temp_dir.path().to_path_buf();

        // Create and populate AbiRepository
        if let Some((_, abi_name, abi_json_str)) = &abi_to_preload {
            let file_path = abi_dir_path.join(format!("{}.json", abi_name));
            std::fs::write(&file_path, abi_json_str).unwrap();
        }
        let abi_repository = Arc::new(AbiRepository::new(&abi_dir_path).unwrap());

        // Create AbiService and link ABIs
        let abi_service = Arc::new(AbiService::new(Arc::clone(&abi_repository)));
        if let Some((address, abi_name, _)) = abi_to_preload {
            abi_service.link_abi(address, abi_name).unwrap();
        }

        MonitorValidator::new(script_validator, abi_service, template_service, "testnet", actions)
    }

    #[tokio::test]
    async fn test_monitor_validation_success_log_monitor() {
        let actions = vec![create_test_action("action1")];
        let contract_address = "0x0000000000000000000000000000000000000123".parse().unwrap();
        let validator =
            create_monitor_validator(&actions, Some((contract_address, "erc20", erc20_abi_json())));

        // Valid: log monitor accesses log and has address + ABI
        let monitor = create_test_monitor(
            1,
            Some("0x0000000000000000000000000000000000000123"),
            Some("erc20"),
            "log.name == \"Transfer\"",
            vec!["action1".to_string()],
        );
        let result = validator.validate(&monitor);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_monitor_validation_success_tx_monitor() {
        let validator = create_monitor_validator(&[], None);
        // Valid: tx monitor accesses tx, no address/ABI required
        let monitor = create_test_monitor(2, None, None, "tx.value > 0", vec![]);
        let result = validator.validate(&monitor);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_monitor_validation_success_tx_only_log_monitor() {
        let validator = create_monitor_validator(&[], None);
        // Valid: log monitor accesses tx only, no address/ABI required
        let monitor = create_test_monitor(
            3,
            Some("0x0000000000000000000000000000000000000123"),
            None,
            "tx.from == \"0x456\"",
            vec![],
        );
        let result = validator.validate(&monitor);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_requires_address_for_log_access() {
        let contract_address = "0x0000000000000000000000000000000000000123".parse().unwrap();
        let validator =
            create_monitor_validator(&[], Some((contract_address, "erc20", erc20_abi_json())));
        // Invalid: accesses log data but has no address
        let invalid_monitor =
            create_test_monitor(1, None, Some("erc20"), "log.name == \"A\"", vec![]);
        let result = validator.validate(&invalid_monitor);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MonitorValidationError::MonitorRequiresAddress { monitor_name: _ }
        ));
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_requires_abi_for_log_access() {
        let validator = create_monitor_validator(&[], None);
        // Invalid: accesses log data but has no ABI
        let invalid_monitor = create_test_monitor(
            1,
            Some("0x0000000000000000000000000000000000000123"),
            None,
            "log.name == \"A\"",
            vec![],
        );
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        if let Err(MonitorValidationError::MonitorRequiresAbi { monitor_name, reason }) = result {
            assert!(monitor_name.contains("Test Monitor 1"));
            assert!(
                reason.contains(
                    "ABI is required for contract-specific log monitoring to decode logs."
                )
            );
        } else {
            panic!("Expected MonitorValidationError::MonitorRequiresAbi");
        }
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_invalid_address_for_log_access() {
        let contract_address = "0x0000000000000000000000000000000000000123".parse().unwrap();
        let validator =
            create_monitor_validator(&[], Some((contract_address, "erc20", erc20_abi_json())));
        // Invalid: address is not a valid hex string, and log data is accessed
        let invalid_addr = "not-a-valid-address";
        let invalid_monitor =
            create_test_monitor(1, Some(invalid_addr), Some("erc20"), "log.name == \"A\"", vec![]);
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MonitorValidationError::InvalidAddress { monitor_name: _, address: _ }
        ));
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_invalid_network() {
        let validator = create_monitor_validator(&[], None);
        // Invalid: monitor is on a different network ("other_network")
        let mut invalid_monitor = create_test_monitor(
            1,
            Some("0x0000000000000000000000000000000000000123"),
            Some("erc20"),
            "log.name == \"A\"",
            vec![],
        );
        invalid_monitor.network = "other_network".to_string(); // Set to a different network

        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MonitorValidationError::InvalidNetwork {
                monitor_name: _,
                expected_network: _,
                actual_network: _
            }
        ));
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_unknown_action() {
        let actions = vec![create_test_action("existing_action")];
        let validator = create_monitor_validator(&actions, None);
        let invalid_monitor =
            create_test_monitor(1, None, None, "tx.value > 0", vec!["unknown_action".to_string()]);
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MonitorValidationError::UnknownAction { monitor_name: _, action_name: _ }
        ));
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_script_syntax_error() {
        let contract_address = "0x0000000000000000000000000000000000000123".parse().unwrap();
        let validator =
            create_monitor_validator(&[], Some((contract_address, "erc20", erc20_abi_json())));
        // Invalid script: syntax error
        let invalid_monitor = create_test_monitor(
            1,
            Some("0x0000000000000000000000000000000000000123"),
            Some("erc20"),
            "log.name == (", // Syntax error
            vec![],
        );
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MonitorValidationError::ScriptError { monitor_name: _, error: _ }
        ));
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_script_invalid_field_access() {
        let contract_address = "0x0000000000000000000000000000000000000123".parse().unwrap();
        let validator =
            create_monitor_validator(&[], Some((contract_address, "erc20", erc20_abi_json())));
        // Invalid script: accesses a non-existent field
        let invalid_monitor = create_test_monitor(
            1,
            Some("0x0000000000000000000000000000000000000123"),
            Some("erc20"),
            "tx.non_existent_field == 1",
            vec![],
        );
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MonitorValidationError::ScriptError { monitor_name: _, error: _ }
        ));
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_script_invalid_return_type() {
        let contract_address = "0x0000000000000000000000000000000000000123".parse().unwrap();
        let validator =
            create_monitor_validator(&[], Some((contract_address, "erc20", erc20_abi_json())));
        // Invalid script: does not return a boolean
        let invalid_monitor = create_test_monitor(
            1,
            Some("0x0000000000000000000000000000000000000123"),
            Some("erc20"),
            "tx.value", // Not a boolean expression
            vec![],
        );
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MonitorValidationError::ScriptError { monitor_name: _, error: _ }
        ));
    }

    #[tokio::test]
    async fn test_monitor_validation_success_no_log_access_no_abi_no_address() {
        let validator = create_monitor_validator(&[], None);
        // Valid: no log access, so no address/ABI required
        let monitor = create_test_monitor(1, None, None, "tx.value > 100", vec![]);
        assert!(validator.validate(&monitor).is_ok());
    }

    #[tokio::test]
    async fn test_monitor_validation_success_log_access_with_abi_and_address() {
        let contract_address = "0x0000000000000000000000000000000000000123".parse().unwrap();
        let validator =
            create_monitor_validator(&[], Some((contract_address, "erc20", erc20_abi_json())));
        // Valid: log access, with address and ABI
        let monitor = create_test_monitor(
            1,
            Some("0x0000000000000000000000000000000000000123"),
            Some("erc20"),
            "log.name == \"Transfer\"",
            vec![],
        );
        let result = validator.validate(&monitor);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_requires_abi_for_log_access_no_abi_file() {
        let contract_address = "0x0000000000000000000000000000000000000123".parse().unwrap();
        let validator =
            create_monitor_validator(&[], Some((contract_address, "erc20", erc20_abi_json())));
        // Invalid: accesses log data, has address, but no abi file specified in monitor
        // config
        let invalid_monitor = create_test_monitor(
            1,
            Some("0x0000000000000000000000000000000000000123"),
            None, // No ABI file specified in monitor config
            "log.name == \"A\"",
            vec![],
        );
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        if let Err(MonitorValidationError::MonitorRequiresAbi { monitor_name, reason }) = result {
            assert!(monitor_name.contains("Test Monitor 1"));
            assert!(
                reason.contains(
                    "ABI is required for contract-specific log monitoring to decode logs."
                )
            );
        } else {
            panic!("Expected MonitorValidationError::MonitorRequiresAbi");
        }
    }

    #[tokio::test]
    async fn test_monitor_validation_success_with_abi_name_even_if_not_linked() {
        let contract_address = "0x0000000000000000000000000000000000000123".parse().unwrap();
        // Create validator without linking the ABI to the address
        let validator =
            create_monitor_validator(&[], Some((contract_address, "erc20", erc20_abi_json())));

        // Valid: accesses log data, has address and abi name
        // The ABI can be retrieved by name even if not linked to the address
        let valid_monitor = create_test_monitor(
            1,
            Some("0x0000000000000000000000000000000000000123"),
            Some("erc20"), // ABI name provided
            "log.name == \"Transfer\"",
            vec![],
        );
        // Manually remove the linked ABI to simulate the scenario
        validator.abi_service.remove_abi(&contract_address);

        let result = validator.validate(&valid_monitor);

        // Should succeed because we can get ABI by name
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_requires_address_for_log_access_no_address() {
        let contract_address = "0x0000000000000000000000000000000000000123".parse().unwrap();
        let validator =
            create_monitor_validator(&[], Some((contract_address, "erc20", erc20_abi_json())));
        // Invalid: accesses log data, has abi file, but no address specified
        let invalid_monitor =
            create_test_monitor(1, None, Some("erc20"), "log.name == \"A\"", vec![]);
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MonitorValidationError::MonitorRequiresAddress { monitor_name: _ }
        ));
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_invalid_address_no_log_access() {
        let validator = create_monitor_validator(&[], None);
        // Invalid: address is not a valid hex string, but no log data is accessed
        let invalid_addr = "not-a-valid-address";
        let invalid_monitor =
            create_test_monitor(1, Some(invalid_addr), None, "tx.value > 0", vec![]);
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MonitorValidationError::InvalidAddress { monitor_name: _, address: _ }
        ));
    }

    #[tokio::test]
    async fn test_monitor_validation_success_global_log_monitor() {
        let validator =
            create_monitor_validator(&[], Some((Address::default(), "erc20", erc20_abi_json())));
        // Valid: global log monitor accesses log and has address: "all" + ABI
        let monitor =
            create_test_monitor(1, Some("all"), Some("erc20"), "log.name == \"Transfer\"", vec![]);
        let result = validator.validate(&monitor);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_global_log_monitor_requires_abi() {
        let validator = create_monitor_validator(&[], None);
        // Invalid: global log monitor accesses log but has no ABI
        let invalid_monitor =
            create_test_monitor(1, Some("all"), None, "log.name == \"A\"", vec![]);
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        if let Err(MonitorValidationError::MonitorRequiresAbi { monitor_name, reason }) = result {
            assert!(monitor_name.contains("Test Monitor 1"));
            assert!(reason.contains(
                "ABI is required for global log monitoring (address: all) to decode logs."
            ));
        } else {
            panic!("Expected MonitorValidationError::MonitorRequiresAbi");
        }
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_global_log_monitor_abi_not_retrieved() {
        let validator = create_monitor_validator(&[], None);
        // Invalid: global log monitor accesses log, has abi name, but ABI is not loaded
        let invalid_monitor = create_test_monitor(
            1,
            Some("all"),
            Some("nonexistent_abi"),
            "log.name == \"A\"",
            vec![],
        );
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        if let Err(MonitorValidationError::MonitorRequiresAbi { monitor_name, reason }) = result {
            assert!(monitor_name.contains("Test Monitor 1"));
            assert!(reason.contains(
                "ABI 'nonexistent_abi' could not be retrieved for global log monitor. Ensure the \
                 ABI is loaded."
            ));
        } else {
            panic!("Expected MonitorValidationError::MonitorRequiresAbi got {:?}", result);
        }
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_script_invalid_abi_field_access() {
        let contract_address = "0x0000000000000000000000000000000000000123".parse().unwrap();
        let validator =
            create_monitor_validator(&[], Some((contract_address, "erc20", erc20_abi_json())));
        // Invalid script: accesses a log.params field not in the ABI
        let invalid_monitor = create_test_monitor(
            1,
            Some("0x0000000000000000000000000000000000000123"),
            Some("erc20"),
            "log.params.non_existent_param == 1",
            vec![],
        );
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MonitorValidationError::ScriptError {
                monitor_name: _,
                error: RhaiScriptValidationError::InvalidAbiField(_)
            }
        ));
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_calldata_aware_no_abi() {
        let contract_address = address!("0x0000000000000000000000000000000000000123");
        let validator =
            create_monitor_validator(&[], Some((contract_address, "erc20", erc20_abi_json())));

        let invalid_monitor = MonitorConfig {
            name: "Test Monitor 1".into(),
            network: "testnet".into(),
            address: contract_address.to_string().into(),
            abi: None,                                          // No ABI provided
            filter_script: "decoded_call.name == \"A\"".into(), // Accesses decoded_call
            actions: vec![],
        };
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        if let Err(MonitorValidationError::InvalidCalldataConfig { monitor_name, reason }) = result
        {
            assert!(monitor_name.contains("Test Monitor 1"));
            assert!(reason.contains(
                "Calldata decoding is enabled, but no ABI is specified. Please provide an ABI \
                 name."
            ));
        } else {
            panic!("Expected MonitorValidationError::InvalidCalldataConfig");
        }
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_calldata_aware_no_address() {
        let validator = create_monitor_validator(&[], None);

        let invalid_monitor = MonitorConfig {
            name: "Test Monitor 1".into(),
            network: "testnet".into(),
            address: None, // No address provided
            abi: Some("erc20".into()),
            filter_script: "decoded_call.name == \"A\"".into(), // Accesses decoded_call
            actions: vec![],
        };
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        if let Err(MonitorValidationError::InvalidCalldataConfig { monitor_name, reason }) = result
        {
            assert!(monitor_name.contains("Test Monitor 1"));
            assert!(reason.contains(
                "Calldata decoding is enabled, but no contract address is specified. Please \
                 provide a contract address."
            ));
        } else {
            panic!("Expected MonitorValidationError::InvalidCalldataConfig");
        }
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_calldata_global_monitor() {
        let validator = create_monitor_validator(&[], None);

        let invalid_monitor = MonitorConfig {
            name: "Test Monitor 1".into(),
            network: "testnet".into(),
            address: Some("all".into()), // Global monitor
            abi: Some("erc20".into()),
            filter_script: "decoded_call.name == \"A\"".into(),
            actions: vec![],
        };
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        if let Err(MonitorValidationError::InvalidCalldataConfig { monitor_name, reason }) = result
        {
            assert!(monitor_name.contains("Test Monitor 1"));
            assert!(reason.contains(
                "Calldata decoding cannot be enabled for global monitors (address: all). Please \
                 specify a concrete contract address."
            ));
        } else {
            panic!("Expected MonitorValidationError::InvalidCalldataConfig");
        }
    }

    #[tokio::test]
    async fn test_monitor_validation_success_calldata_enabled_with_abi_and_address() {
        let contract_address = address!("0x1234567890abcdef1234567890abcdef12345678");
        let validator =
            create_monitor_validator(&[], Some((contract_address, "erc20", erc20_abi_json())));

        let invalid_monitor = MonitorConfig {
            name: "Test Monitor 1".into(),
            network: "testnet".into(),
            address: Some(contract_address.to_string()),
            abi: Some("erc20".into()),
            filter_script: "log.name == \"A\"".into(),
            actions: vec![],
        };

        let result = validator.validate(&invalid_monitor);

        assert!(result.is_ok(), "Expected validation to succeed but got error: {:?}", result);
    }

    #[tokio::test]
    async fn test_monitor_validation_action_template() {
        let contract_address = address!("0x1234567890abcdef1234567890abcdef12345678");
        let actions = vec![
            ActionBuilder::new("valid_action")
                .discord_config("http://localhost")
                .message("Tx hash: {{ transaction.hash }}")
                .build(),
            ActionBuilder::new("invalid_action")
                .discord_config("http://localhost")
                .message("Invalid field: {{ transaction.invalid_field }}")
                .build(),
        ];
        let validator =
            create_monitor_validator(&actions, Some((contract_address, "erc20", erc20_abi_json())));

        // Test case 1: Valid template
        let monitor_with_valid_action = create_test_monitor(
            1,
            Some(&contract_address.to_string()),
            Some("erc20"),
            "true",
            vec!["valid_action".to_string()],
        );
        let result = validator.validate(&monitor_with_valid_action);
        assert!(result.is_ok(), "Expected validation to succeed but got error: {:?}", result);

        // Test case 2: Invalid template
        let monitor_with_invalid_action = create_test_monitor(
            2,
            Some(&contract_address.to_string()),
            Some("erc20"),
            "true",
            vec!["invalid_action".to_string()],
        );
        let result = validator.validate(&monitor_with_invalid_action);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MonitorValidationError::InvalidActionTemplate { .. }
        ));
    }

    #[tokio::test]
    async fn test_monitor_validation_action_template_log_and_calldata() {
        let contract_address = address!("0x1234567890abcdef1234567890abcdef12345678");
        let actions = vec![
            ActionBuilder::new("valid_log_action")
                .discord_config("http://localhost")
                .message("Log name: {{ log.name }}, Param: {{ log.params.from }}")
                .build(),
            ActionBuilder::new("invalid_log_action")
                .discord_config("http://localhost")
                .message("Invalid field: {{ log.params.invalid_param }}")
                .build(),
            ActionBuilder::new("valid_calldata_action")
                .discord_config("http://localhost")
                .message("Call name: {{ decoded_call.name }}, Input: {{ decoded_call.inputs._to }}")
                .build(),
            ActionBuilder::new("invalid_calldata_action")
                .discord_config("http://localhost")
                .message("Invalid field: {{ decoded_call.inputs.invalid_input }}")
                .build(),
        ];
        let validator =
            create_monitor_validator(&actions, Some((contract_address, "erc20", erc20_abi_json())));

        // Test case 1: Valid log template
        let monitor_with_valid_log_action = create_test_monitor(
            1,
            Some(&contract_address.to_string()),
            Some("erc20"),
            "log.name == \"Transfer\"",
            vec!["valid_log_action".to_string()],
        );
        let result = validator.validate(&monitor_with_valid_log_action);
        assert!(result.is_ok(), "Expected validation to succeed but got error: {:?}", result);

        // Test case 2: Invalid log template
        let monitor_with_invalid_log_action = create_test_monitor(
            2,
            Some(&contract_address.to_string()),
            Some("erc20"),
            "log.name == \"Transfer\"",
            vec!["invalid_log_action".to_string()],
        );
        let result = validator.validate(&monitor_with_invalid_log_action);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MonitorValidationError::InvalidActionTemplate { .. }
        ));

        // Test case 3: Valid calldata template
        let monitor_with_valid_calldata_action = create_test_monitor(
            3,
            Some(&contract_address.to_string()),
            Some("erc20"),
            "decoded_call.name == \"transfer\"",
            vec!["valid_calldata_action".to_string()],
        );
        let result = validator.validate(&monitor_with_valid_calldata_action);
        assert!(result.is_ok(), "Expected validation to succeed but got error: {:?}", result);

        // Test case 4: Invalid calldata template
        let monitor_with_invalid_calldata_action = create_test_monitor(
            4,
            Some(&contract_address.to_string()),
            Some("erc20"),
            "decoded_call.name == \"transfer\"",
            vec!["invalid_calldata_action".to_string()],
        );
        let result = validator.validate(&monitor_with_invalid_calldata_action);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MonitorValidationError::InvalidActionTemplate { .. }
        ));
    }
}
