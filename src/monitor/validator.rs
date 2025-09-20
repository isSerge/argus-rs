//! Module for loading and validating monitor configurations.

use std::sync::Arc;

use alloy::{json_abi::JsonAbi, primitives::Address};
use thiserror::Error;

use crate::{
    abi::AbiService,
    config::ActionConfig,
    engine::rhai::{RhaiScriptValidationError, RhaiScriptValidationResult, RhaiScriptValidator},
    models::{monitor::MonitorConfig, notifier::NotifierConfig},
};

/// A validator for monitor configurations.
pub struct MonitorValidator<'a> {
    /// The application network ID.
    network_id: &'a str,

    /// The list of available notifiers.
    notifiers: &'a [NotifierConfig],

    /// The script validator for validating Rhai scripts.
    script_validator: RhaiScriptValidator,

    /// The ABI service for retrieving contract ABIs.
    abi_service: Arc<AbiService>,

    /// The list of available actions.
    actions: &'a [ActionConfig],
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

    /// The monitor references a notifier that does not exist.
    #[error("Monitor '{monitor_name}' references an unknown notifier: '{notifier_name}'")]
    UnknownNotifier {
        /// The name of the monitor.
        monitor_name: String,
        /// The name of the unknown notifier.
        notifier_name: String,
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

    /// An action referenced in the monitor's `on_match` does not exist.
    #[error("Monitor '{monitor_name}' references an unknown action: '{action_name}'")]
    UnknownAction {
        /// The name of the monitor.
        monitor_name: String,
        /// The name of the unknown action.
        action_name: String,
    },
}

impl<'a> MonitorValidator<'a> {
    /// Creates a new `MonitorValidator` for the specified network ID.
    pub fn new(
        script_validator: RhaiScriptValidator,
        abi_service: Arc<AbiService>,
        network_id: &'a str,
        notifiers: &'a [NotifierConfig],
        actions: &'a [ActionConfig],
    ) -> Self {
        MonitorValidator { network_id, notifiers, script_validator, abi_service, actions }
    }

    /// Validates the given monitor configuration.
    pub fn validate(&self, monitor: &MonitorConfig) -> Result<(), MonitorValidationError> {
        tracing::debug!(monitor = ?monitor, "Validating monitor configuration...");

        // Check if network id matches the monitor's network.
        if monitor.network != self.network_id {
            return Err(MonitorValidationError::InvalidNetwork {
                monitor_name: monitor.name.clone(),
                expected_network: self.network_id.to_string(),
                actual_network: monitor.network.clone(),
            });
        }

        self.validate_actions(monitor)?;
        self.validate_notifiers(monitor)?;

        let (parsed_address, is_global_log_monitor) = self.parse_and_validate_address(monitor)?;

        let abi_json = self.get_monitor_abi_json(monitor, parsed_address, is_global_log_monitor);

        let script_validation_result = self
            .script_validator
            .validate_script(&monitor.filter_script, abi_json.as_deref())
            .map_err(|e| MonitorValidationError::ScriptError {
                monitor_name: monitor.name.clone(),
                error: e,
            })?;

        self.validate_script_and_abi_rules(
            monitor,
            &script_validation_result,
            is_global_log_monitor,
            abi_json.is_some(),
        )?;

        if script_validation_result.ast_analysis.accesses_call_variable {
            self.validate_calldata_rules(monitor)?;
        }

        Ok(())
    }

    /// Validates that all notifiers referenced by the monitor exist.
    fn validate_notifiers(&self, monitor: &MonitorConfig) -> Result<(), MonitorValidationError> {
        if monitor.notifiers.is_empty() {
            tracing::warn!(
                monitor_name = monitor.name,
                "Monitor has no notifiers configured and will not send any notifications."
            );
        }

        // Check if all notifiers exist.
        for notifier_name in &monitor.notifiers {
            if !self.notifiers.iter().any(|n| n.name == *notifier_name) {
                return Err(MonitorValidationError::UnknownNotifier {
                    monitor_name: monitor.name.clone(),
                    notifier_name: notifier_name.clone(),
                });
            }
        }

        Ok(())
    }

    /// Validates that all actions referenced by the monitor exist.
    fn validate_actions(&self, monitor: &MonitorConfig) -> Result<(), MonitorValidationError> {
        if let Some(on_match) = &monitor.on_match {
            for action_name in on_match {
                if !self.actions.iter().any(|a| a.name == *action_name) {
                    return Err(MonitorValidationError::UnknownAction {
                        monitor_name: monitor.name.clone(),
                        action_name: action_name.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Validates the calldata decoding rules for the monitor.
    fn validate_calldata_rules(
        &self,
        monitor: &MonitorConfig,
    ) -> Result<(), MonitorValidationError> {
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

    /// Parses and validates the address for the monitor.
    /// Returns the parsed address (if any) and a flag indicating if it's a
    /// global log monitor.
    fn parse_and_validate_address(
        &self,
        monitor: &MonitorConfig,
    ) -> Result<(Option<Address>, bool), MonitorValidationError> {
        // Parse and validate the contract address if provided.
        let mut parsed_address: Option<Address> = None;
        let mut is_global_log_monitor = false;

        if let Some(address_str) = &monitor.address {
            if address_str.eq_ignore_ascii_case("all") {
                is_global_log_monitor = true;
                // For global monitors, parsed_address remains None.
            } else {
                match address_str.parse::<Address>() {
                    Ok(addr) => parsed_address = Some(addr),
                    Err(_) => {
                        return Err(MonitorValidationError::InvalidAddress {
                            monitor_name: monitor.name.clone(),
                            address: address_str.clone(),
                        });
                    }
                }
            }
        }
        Ok((parsed_address, is_global_log_monitor))
    }

    /// Gets the ABI JSON for the monitor based on its configuration.
    /// For contract-specific monitors, it's Some if address and abi path are
    /// provided and address is valid. For global log monitors, it's Some if
    /// abi path is provided.
    fn get_monitor_abi_json(
        &self,
        monitor: &MonitorConfig,
        parsed_address: Option<Address>,
        is_global_log_monitor: bool,
    ) -> Option<Arc<JsonAbi>> {
        if let Some(address) = parsed_address {
            if monitor.abi.is_some() {
                self.abi_service.get_abi(address).map(|c| c.abi.clone())
            } else {
                None // No ABI name provided for contract-specific, so no ABI expected for script validation
            }
        } else if is_global_log_monitor {
            if let Some(abi_name) = &monitor.abi {
                // For global log monitors, we don't have a specific address to link the ABI to,
                // so we just try to get the ABI by name.
                self.abi_service.get_abi_by_name(abi_name)
            } else {
                None // No ABI name provided for global log monitor
            }
        } else {
            None // No address and not a global log monitor, so no ABI expected for script validation
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use alloy::primitives::{Address, address};
    use tempfile::tempdir;

    use crate::{
        abi::{AbiService, repository::AbiRepository},
        config::{ActionConfig, RhaiConfig},
        engine::rhai::{RhaiCompiler, RhaiScriptValidationError, RhaiScriptValidator},
        models::{
            NotificationMessage,
            monitor::MonitorConfig,
            notifier::{NotifierConfig, NotifierTypeConfig, WebhookConfig},
        },
        monitor::{MonitorValidationError, MonitorValidator},
        test_helpers::erc20_abi_json,
    };

    fn create_test_monitor(
        id: i64,
        address: Option<&str>,
        abi: Option<&str>,
        script: &str,
        notifiers: Vec<String>,
    ) -> MonitorConfig {
        MonitorConfig {
            name: format!("Test Monitor {id}"),
            network: "testnet".to_string(),
            address: address.map(String::from),
            abi: abi.map(String::from),
            filter_script: script.to_string(),
            notifiers,
            on_match: None,
        }
    }

    fn create_test_notifier(name: &str) -> NotifierConfig {
        NotifierConfig {
            name: name.to_string(),
            config: NotifierTypeConfig::Webhook(WebhookConfig {
                url: "http://localhost".to_string(),
                message: NotificationMessage::default(),
                ..Default::default()
            }),
            policy: None,
        }
    }

    fn create_monitor_validator<'a>(
        notifiers: &'a [NotifierConfig],
        actions: &'a [ActionConfig],
        abi_to_preload: Option<(Address, &'static str, &'static str)>,
    ) -> MonitorValidator<'a> {
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config));
        let script_validator = RhaiScriptValidator::new(compiler);

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

        MonitorValidator::new(script_validator, abi_service, "testnet", notifiers, actions)
    }

    #[tokio::test]
    async fn test_monitor_validation_success_log_monitor() {
        let notifiers = vec![create_test_notifier("notifier1")];
        let contract_address = "0x0000000000000000000000000000000000000123".parse().unwrap();
        let validator = create_monitor_validator(
            &notifiers,
            &[],
            Some((contract_address, "erc20", erc20_abi_json())),
        );

        // Valid: log monitor accesses log and has address + ABI
        let monitor = create_test_monitor(
            1,
            Some("0x0000000000000000000000000000000000000123"),
            Some("erc20"),
            "log.name == \"Transfer\"",
            vec!["notifier1".to_string()],
        );
        let result = validator.validate(&monitor);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_monitor_validation_success_tx_monitor() {
        let validator = create_monitor_validator(&[], &[], None);
        // Valid: tx monitor accesses tx, no address/ABI required
        let monitor = create_test_monitor(2, None, None, "tx.value > 0", vec![]);
        let result = validator.validate(&monitor);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_monitor_validation_success_tx_only_log_monitor() {
        let validator = create_monitor_validator(&[], &[], None);
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
            create_monitor_validator(&[], &[], Some((contract_address, "erc20", erc20_abi_json())));
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
        let validator = create_monitor_validator(&[], &[], None);
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
            create_monitor_validator(&[], &[], Some((contract_address, "erc20", erc20_abi_json())));
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
        let validator = create_monitor_validator(&[], &[], None);
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
    async fn test_monitor_validation_failure_unknown_notifier() {
        let notifiers = vec![create_test_notifier("existing_notifier")];
        let validator = create_monitor_validator(&notifiers, &[], None);
        let invalid_monitor = create_test_monitor(
            1,
            None,
            None,
            "tx.value > 0",
            vec!["unknown_notifier".to_string()],
        );
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MonitorValidationError::UnknownNotifier { monitor_name: _, notifier_name: _ }
        ));
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_script_syntax_error() {
        let contract_address = "0x0000000000000000000000000000000000000123".parse().unwrap();
        let validator =
            create_monitor_validator(&[], &[], Some((contract_address, "erc20", erc20_abi_json())));
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
            create_monitor_validator(&[], &[], Some((contract_address, "erc20", erc20_abi_json())));
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
            create_monitor_validator(&[], &[], Some((contract_address, "erc20", erc20_abi_json())));
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
        let validator = create_monitor_validator(&[], &[], None);
        // Valid: no log access, so no address/ABI required
        let monitor = create_test_monitor(1, None, None, "tx.value > 100", vec![]);
        assert!(validator.validate(&monitor).is_ok());
    }

    #[tokio::test]
    async fn test_monitor_validation_success_log_access_with_abi_and_address() {
        let contract_address = "0x0000000000000000000000000000000000000123".parse().unwrap();
        let validator =
            create_monitor_validator(&[], &[], Some((contract_address, "erc20", erc20_abi_json())));
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
            create_monitor_validator(&[], &[], Some((contract_address, "erc20", erc20_abi_json())));
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
    async fn test_monitor_validation_failure_requires_abi_for_log_access_abi_not_linked() {
        let contract_address = "0x0000000000000000000000000000000000000123".parse().unwrap();
        // Create validator without linking the ABI to the address
        let validator =
            create_monitor_validator(&[], &[], Some((contract_address, "erc20", erc20_abi_json())));

        // Invalid: accesses log data, has address and abi name, but ABI is not linked
        let invalid_monitor = create_test_monitor(
            1,
            Some("0x0000000000000000000000000000000000000123"),
            Some("erc20"), // ABI name provided
            "log.name == \"A\"",
            vec![],
        );
        // Manually remove the linked ABI to simulate the failure condition
        validator.abi_service.remove_abi(&contract_address);

        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        if let Err(MonitorValidationError::MonitorRequiresAbi { monitor_name, reason }) = result {
            assert!(monitor_name.contains("Test Monitor 1"));
            assert!(reason.contains(
                "ABI 'erc20' could not be retrieved for address \
                 '0x0000000000000000000000000000000000000123'. Ensure the ABI is loaded and \
                 linked."
            ));
        } else {
            panic!("Expected MonitorValidationError::MonitorRequiresAbi");
        }
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_requires_address_for_log_access_no_address() {
        let contract_address = "0x0000000000000000000000000000000000000123".parse().unwrap();
        let validator =
            create_monitor_validator(&[], &[], Some((contract_address, "erc20", erc20_abi_json())));
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
        let validator = create_monitor_validator(&[], &[], None);
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
        let validator = create_monitor_validator(
            &[],
            &[],
            Some((Address::default(), "erc20", erc20_abi_json())),
        );
        // Valid: global log monitor accesses log and has address: "all" + ABI
        let monitor =
            create_test_monitor(1, Some("all"), Some("erc20"), "log.name == \"Transfer\"", vec![]);
        let result = validator.validate(&monitor);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_global_log_monitor_requires_abi() {
        let validator = create_monitor_validator(&[], &[], None);
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
        let validator = create_monitor_validator(&[], &[], None);
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
            create_monitor_validator(&[], &[], Some((contract_address, "erc20", erc20_abi_json())));
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
            create_monitor_validator(&[], &[], Some((contract_address, "erc20", erc20_abi_json())));

        let invalid_monitor = MonitorConfig {
            name: "Test Monitor 1".into(),
            network: "testnet".into(),
            address: contract_address.to_string().into(),
            abi: None,                                          // No ABI provided
            filter_script: "decoded_call.name == \"A\"".into(), // Accesses decoded_call
            ..Default::default()
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
        let validator = create_monitor_validator(&[], &[], None);

        let invalid_monitor = MonitorConfig {
            name: "Test Monitor 1".into(),
            network: "testnet".into(),
            address: None, // No address provided
            abi: Some("erc20".into()),
            filter_script: "decoded_call.name == \"A\"".into(), // Accesses decoded_call
            notifiers: vec![],
            on_match: None,
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
        let validator = create_monitor_validator(&[], &[], None);

        let invalid_monitor = MonitorConfig {
            name: "Test Monitor 1".into(),
            network: "testnet".into(),
            address: Some("all".into()), // Global monitor
            abi: Some("erc20".into()),
            filter_script: "decoded_call.name == \"A\"".into(),
            ..Default::default()
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
            create_monitor_validator(&[], &[], Some((contract_address, "erc20", erc20_abi_json())));

        let invalid_monitor = MonitorConfig {
            name: "Test Monitor 1".into(),
            network: "testnet".into(),
            address: Some(contract_address.to_string()),
            abi: Some("erc20".into()),
            filter_script: "log.name == \"A\"".into(),
            ..Default::default()
        };

        let result = validator.validate(&invalid_monitor);

        assert!(result.is_ok(), "Expected validation to succeed but got error: {:?}", result);
    }

    #[tokio::test]
    async fn test_monitor_validation_on_match_action_validation() {
        let temp_action_file = tempfile::NamedTempFile::with_suffix(".js").unwrap();
        let action_file_path = temp_action_file.path().to_path_buf();

        let actions = vec![ActionConfig { name: "test_action".into(), file: action_file_path }];
        let validator = create_monitor_validator(&[], &actions, None);

        let monitor_with_no_actions = MonitorConfig {
            name: "Monitor with no actions".into(),
            network: "testnet".into(),
            filter_script: "tx.value > 0".into(),
            on_match: None,
            ..Default::default()
        };

        let monitor_with_actions = MonitorConfig {
            name: "Monitor with actions".into(),
            network: "testnet".into(),
            filter_script: "tx.value > 0".into(),
            on_match: Some(vec!["test_action".to_string()]),
            ..Default::default()
        };

        let monitor_with_unknown_action = MonitorConfig {
            name: "Monitor with unknown action".into(),
            network: "testnet".into(),
            filter_script: "tx.value > 0".into(),
            on_match: Some(vec!["unknown_action".to_string()]),
            ..Default::default()
        };

        // Validate monitor without actions - should pass
        let result_no_actions = validator.validate(&monitor_with_no_actions);
        assert!(
            result_no_actions.is_ok(),
            "Expected validation to succeed but got error: {:?}",
            result_no_actions.unwrap_err()
        );

        // Validate monitor with actions - should pass as action exists
        let result_with_actions = validator.validate(&monitor_with_actions);
        assert!(
            result_with_actions.is_ok(),
            "Expected validation to succeed but got error: {:?}",
            result_with_actions.unwrap_err()
        );

        // Validate monitor with unknown action - should fail
        let result_unknown_action = validator.validate(&monitor_with_unknown_action);
        assert!(result_unknown_action.is_err(), "Expected validation to fail but it succeeded");
        if let Err(MonitorValidationError::UnknownAction { monitor_name, action_name }) =
            result_unknown_action
        {
            assert!(monitor_name.contains("Monitor with unknown action"));
            assert_eq!(action_name, "unknown_action");
        } else {
            panic!("Expected MonitorValidationError::UnknownAction");
        }
    }
}
