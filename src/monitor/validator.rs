//! Module for loading and validating monitor configurations.

use alloy::primitives::Address;
use thiserror::Error;

use crate::{models::monitor::Monitor};

/// A validator for monitor configurations.
pub struct MonitorValidator<'a> {
    /// The application network ID.
    network_id: &'a str,
}

/// An error that occurs during monitor validation.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum MonitorValidationError {
    /// A monitor that accesses log data does not have a contract address specified.
    #[error(
        "Monitor '{monitor_name}' accesses log data ('log.*') but is not tied to a specific contract address. Please provide an 'address' for this monitor."
    )]
    MonitorRequiresAddress {
        /// The name of the monitor that failed validation.
        monitor_name: String,
    },

    /// A monitor that accesses log data does not have an ABI defined.
    #[error(
        "Monitor '{monitor_name}' accesses log data but does not have an ABI defined. Please provide an 'abi' file path."
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
        "Monitor '{monitor_name}' is configured for network '{expected_network}', but it is actually on network '{actual_network}'."
    )]
    InvalidNetwork {
        /// The name of the monitor that failed validation.
        monitor_name: String,
        /// The expected network ID.
        expected_network: String,
        /// The actual network ID of the monitor.
        actual_network: String,
    },
}

impl<'a> MonitorValidator<'a> {
    /// Creates a new `MonitorValidator` for the specified network ID.
    pub fn new(network_id: &'a str) -> Self {
        MonitorValidator { network_id }
    }

    /// Validates the given monitor configuration.
    pub fn validate(&self, monitor: &Monitor) -> Result<(), MonitorValidationError> {
        // Check if network id matches the monitor's network.
        if monitor.network != self.network_id {
            return Err(MonitorValidationError::InvalidNetwork {
                monitor_name: monitor.name.clone(),
                expected_network: self.network_id.to_string(),
                actual_network: monitor.network.clone(),
            });
        }

        if monitor.filter_script.contains("log.") {
            // If the monitor accesses log data, it must have an address and ABI.
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
                address
                    .parse::<Address>()
                    .map_err(|_| MonitorValidationError::InvalidAddress {
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
    use super::*;

    fn create_test_monitor(
        id: i64,
        address: Option<&str>,
        abi: Option<&str>,
        script: &str,
    ) -> Monitor {
        let mut monitor = Monitor::from_config(
            format!("Test Monitor {id}"),
            "testnet".to_string(),
            address.map(String::from),
            abi.map(String::from),
            script.to_string(),
        );
        monitor.id = id;
        monitor
    }

    #[tokio::test]
    async fn test_monitor_validation_success() {
        // Valid: log monitor accesses log and has address + ABI
        let monitor1 = create_test_monitor(
            1,
            Some("0x0000000000000000000000000000000000000123"),
            Some("abi.json"),
            "log.name == 'A'",
        );
        // Valid: tx monitor accesses tx
        let monitor2 = create_test_monitor(2, None, None, "tx.value > 0");
        // Valid: log monitor accesses tx only
        let monitor3 = create_test_monitor(
            3,
            Some("0x0000000000000000000000000000000000000123"),
            None,
            "tx.from == '0x456'",
        );

        let validator = MonitorValidator::new("testnet");
        assert!(validator.validate(&monitor1).is_ok());
        assert!(validator.validate(&monitor2).is_ok());
        assert!(validator.validate(&monitor3).is_ok());
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_requires_address() {
        // Invalid: accesses log data but has no address
        let invalid_monitor = create_test_monitor(1, None, None, "log.name == 'A'");
        let validator = MonitorValidator::new("testnet");
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            MonitorValidationError::MonitorRequiresAddress {
                monitor_name: invalid_monitor.name
            }
        );
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_requires_abi() {
        // Invalid: accesses log data but has no ABI
        let invalid_monitor = create_test_monitor(
            1,
            Some("0x0000000000000000000000000000000000000123"),
            None,
            "log.name == 'A'",
        );
        let validator = MonitorValidator::new("testnet");
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            MonitorValidationError::MonitorRequiresAbi {
                monitor_name: invalid_monitor.name
            }
        );
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_invalid_address() {
        // Invalid: address is not a valid hex string
        let invalid_addr = "not-a-valid-address";
        let invalid_monitor =
            create_test_monitor(1, Some(invalid_addr), Some("abi.json"), "log.name == 'A'");
        let validator = MonitorValidator::new("testnet");
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            MonitorValidationError::InvalidAddress {
                monitor_name: invalid_monitor.name,
                address: invalid_addr.to_string()
            }
        );
    }

    #[tokio::test]
    async fn test_monitor_validation_failure_invalid_network() {
        // Invalid: monitor is on a different network ("testnet")
        let invalid_monitor = create_test_monitor(
            1,
            Some("0x0000000000000000000000000000000000000123"),
            Some("abi.json"),
            "log.name == 'A'",
        );
        let validator = MonitorValidator::new("mainnet"); // Expecting "mainnet" but monitor is "testnet"
        let result = validator.validate(&invalid_monitor);

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            MonitorValidationError::InvalidNetwork {
                monitor_name: invalid_monitor.name,
                expected_network: "mainnet".to_string(),
                actual_network: invalid_monitor.network.clone(),
            }
        );
    }
}
