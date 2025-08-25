//! Module for loading and validating monitor configurations.

use std::sync::Arc;

use alloy::primitives::Address;
use thiserror::Error;

use crate::{
    abi::AbiService, engine::rhai::{RhaiScriptValidationError, RhaiScriptValidator}, models::{monitor::MonitorConfig, notifier::NotifierConfig}
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
}

/// An error that occurs during monitor validation.
#[derive(Debug, Error)]
pub enum MonitorValidationError {
    /// A monitor that accesses log data does not have a contract address
    /// specified.
    #[error(
        "Monitor '{monitor_name}' accesses log data ('log.*') but is not tied to a specific \
         contract address. Please provide an 'address' for this monitor."
    )]
    MonitorRequiresAddress {
        /// The name of the monitor that failed validation.
        monitor_name: String,
    },

    /// A monitor that accesses log data does not have an ABI defined.
    #[error(
        "Monitor '{monitor_name}' accesses log data but does not have an ABI defined. Please \
         provide an 'abi' file path."
    )]
    MonitorRequiresAbi {
        /// The name of the monitor that failed validation.
        monitor_name: String,
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
        error: RhaiScriptValidationError 
    },
}

impl<'a> MonitorValidator<'a> {
    /// Creates a new `MonitorValidator` for the specified network ID.
    pub fn new(
        script_validator: RhaiScriptValidator,
        abi_service: Arc<AbiService>,
        network_id: &'a str,
        notifiers: &'a [NotifierConfig],
    ) -> Self {
        MonitorValidator { network_id, notifiers, script_validator, abi_service }
    }

    /// Validates the given monitor configuration.
    pub fn validate(&self, monitor: &MonitorConfig) -> Result<(), MonitorValidationError> {
        // Check if network id matches the monitor's network.
        if monitor.network != self.network_id {
            return Err(MonitorValidationError::InvalidNetwork {
                monitor_name: monitor.name.clone(),
                expected_network: self.network_id.to_string(),
                actual_network: monitor.network.clone(),
            });
        }

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

        // Get ABI from ABI service
        let address = monitor.address.as_ref().and_then(|addr| addr.parse::<Address>().ok());
        let cached_contract = self.abi_service.get_abi(&address);
        let abi = cached_contract.and_then(|c| Some(c.abi.clone()));

        // TODO: do something with result
        let validation_result =
            self.script_validator.validate_script(&monitor.filter_script, abi.as_ref()).map_err(|e| {
                MonitorValidationError::ScriptError { monitor_name: monitor.name.clone(), error: e }
            })?;

        // Enforce address and ABI requirements based on script analysis.
        if validation_result.ast_analysis.accesses_log_variable {
            if monitor.address.is_none() {
                return Err(MonitorValidationError::MonitorRequiresAddress {
                    monitor_name: monitor.name.clone(),
                });
            }

            if monitor.abi.is_none() {
                return Err(MonitorValidationError::MonitorRequiresAbi {
                    monitor_name: monitor.name.clone(),
                });
            }

            // Parse the address to ensure it's valid.
            if let Some(address) = &monitor.address {
                address.parse::<Address>().map_err(|_| MonitorValidationError::InvalidAddress {
                    monitor_name: monitor.name.clone(),
                    address: address.clone(),
                })?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        abi::AbiService, config::RhaiConfig, engine::rhai::{RhaiCompiler, RhaiScriptValidator}, models::{
            monitor::MonitorConfig, notifier::{NotifierConfig, NotifierTypeConfig, WebhookConfig}, NotificationMessage
        }, monitor::{MonitorValidationError, MonitorValidator}
    };

    fn create_test_monitor(
        id: i64,
        address: Option<&str>,
        abi: Option<&str>,
        script: &str,
        notifiers: Vec<String>,
    ) -> MonitorConfig {
        MonitorConfig::from_config(
            format!("Test Monitor {id}"),
            "testnet".to_string(),
            address.map(String::from),
            abi.map(String::from),
            script.to_string(),
            notifiers,
        )
    }

    fn create_test_notifier(name: &str) -> NotifierConfig {
        NotifierConfig {
            name: name.to_string(),
            config: NotifierTypeConfig::Webhook(WebhookConfig {
                url: "http://localhost".to_string(),
                message: NotificationMessage::default(),
                ..Default::default()
            }),
        }
    }

    fn create_monitor_validator(notifiers: &[NotifierConfig]) -> MonitorValidator<'_> {
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config));
        let script_validator = RhaiScriptValidator::new(compiler);
        let abi_service = Arc::new(AbiService::new());
        MonitorValidator::new(script_validator, abi_service, "testnet", notifiers)
    }

    #[tokio::test]
    async fn test_monitor_validation_success() {
        let notifiers = vec![create_test_notifier("notifier1")];
        let validator = create_monitor_validator(&notifiers);

        // Valid: log monitor accesses log and has address + ABI
        let monitor1 = create_test_monitor(
            1,
            Some("0x0000000000000000000000000000000000000123"),
            Some("abi.json"),
            "log.name == 'A'",
            vec!["notifier1".to_string()],
        );
        // Valid: tx monitor accesses tx
        let monitor2 = create_test_monitor(2, None, None, "tx.value > 0", vec![]);
        // Valid: log monitor accesses tx only
        let monitor3 = create_test_monitor(
            3,
            Some("0x0000000000000000000000000000000000000123"),
            None,
            "tx.from == '0x456'",
            vec![],
        );

        assert!(validator.validate(&monitor1).is_ok());
        assert!(validator.validate(&monitor2).is_ok());
        assert!(validator.validate(&monitor3).is_ok());
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_requires_address() {
        let validator = create_monitor_validator(&[]);
        // Invalid: accesses log data but has no address
        let invalid_monitor = create_test_monitor(1, None, None, "log.name == 'A'", vec![]);
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), MonitorValidationError::MonitorRequiresAddress { monitor_name: _ }));
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_requires_abi() {
        let validator = create_monitor_validator(&[]);
        // Invalid: accesses log data but has no ABI
        let invalid_monitor = create_test_monitor(
            1,
            Some("0x0000000000000000000000000000000000000123"),
            None,
            "log.name == 'A'",
            vec![],
        );
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), MonitorValidationError::MonitorRequiresAbi { monitor_name: _ }));
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_invalid_address() {
        let validator = create_monitor_validator(&[]);
        // Invalid: address is not a valid hex string
        let invalid_addr = "not-a-valid-address";
        let invalid_monitor =
            create_test_monitor(1, Some(invalid_addr), Some("abi.json"), "log.name == 'A'", vec![]);
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), MonitorValidationError::InvalidAddress { monitor_name: _, address: _ }));
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_invalid_network() {
        let validator = create_monitor_validator(&[]);
        // Invalid: monitor is on a different network ("testnet")
        let invalid_monitor = create_test_monitor(
            1,
            Some("0x0000000000000000000000000000000000000123"),
            Some("abi.json"),
            "log.name == 'A'",
            vec![],
        );
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), MonitorValidationError::InvalidNetwork { monitor_name: _, expected_network: _, actual_network: _ }));
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_unknown_notifier() {
        let notifiers = vec![create_test_notifier("existing_notifier")];
        let validator = create_monitor_validator(&notifiers);
        let invalid_monitor = create_test_monitor(
            1,
            None,
            None,
            "tx.value > 0",
            vec!["unknown_notifier".to_string()],
        );
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), MonitorValidationError::UnknownNotifier { monitor_name: _, notifier_name: _ }));
    }
}
