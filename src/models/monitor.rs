//! This module defines the `Monitor` structure, which represents a blockchain
//! monitor that tracks specific addresses and networks.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Configuration for a monitor, used to create new monitors from config files
/// before they are persisted to the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorConfig {
    /// Name of the monitor
    pub name: String,

    /// Blockchain network the monitor is associated with
    pub network: String,

    /// Address of the monitor (optional)
    #[serde(default)]
    pub address: Option<String>,

    /// ABI of the monitor (optional)
    #[serde(default)]
    pub abi: Option<String>,

    /// Filter script for the monitor
    pub filter_script: String,

    /// Notifiers for the monitor (optional)
    #[serde(default)]
    pub notifiers: Vec<String>,
}

/// Represents a blockchain monitor retrieved from the database.
#[derive(Debug, Clone, FromRow)]
pub struct Monitor {
    /// Unique identifier for the monitor (auto-generated when loading from
    /// config)
    #[sqlx(rename = "monitor_id")]
    pub id: i64,

    /// Name of the monitor
    pub name: String,

    /// The blockchain network this monitor is associated with (e.g., Ethereum)
    pub network: String,

    /// The specific address this monitor is tracking.
    /// If `None`, the monitor will be applied to all transactions (e.g., for
    /// native token transfers).
    pub address: Option<String>,

    /// The ABI (Application Binary Interface) for the contract being monitored.
    /// This is used to decode event logs and call contract methods.
    /// If `None`, the monitor will not decode logs.
    pub abi: Option<String>,

    /// The filter script used to determine relevant blockchain events
    pub filter_script: String,

    /// The notifiers to execute when the filter script matches
    pub notifiers: Vec<String>,

    /// Timestamp when the monitor was created
    pub created_at: DateTime<Utc>,

    /// Timestamp when the monitor was last updated
    pub updated_at: DateTime<Utc>,
}

impl MonitorConfig {
    /// Creates a new monitor from configuration data (without an ID)
    pub fn from_config(
        name: String,
        network: String,
        address: Option<String>,
        abi: Option<String>,
        filter_script: String,
        notifiers: Vec<String>,
    ) -> Self {
        Self { name, network, address, abi, filter_script, notifiers }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monitor_from_config_with_address() {
        let monitor = MonitorConfig::from_config(
            "Test Monitor".to_string(),
            "ethereum".to_string(),
            Some("0x123".to_string()),
            Some("abis/test.json".to_string()),
            "log.name == \"Test\"".to_string(),
            vec!["test-notifier".to_string()],
        );

        assert_eq!(monitor.name, "Test Monitor");
        assert_eq!(monitor.network, "ethereum");
        assert_eq!(monitor.address, Some("0x123".to_string()));
        assert_eq!(monitor.filter_script, "log.name == \"Test\"");
        assert_eq!(monitor.notifiers, vec!["test-notifier".to_string()]);
    }

    #[test]
    fn test_monitor_from_config_without_address() {
        let monitor = MonitorConfig::from_config(
            "Native Transfer Monitor".to_string(),
            "ethereum".to_string(),
            None,
            None,
            "bigint(tx.value) > bigint(1000)".to_string(),
            vec![],
        );

        assert_eq!(monitor.name, "Native Transfer Monitor");
        assert_eq!(monitor.network, "ethereum");
        assert_eq!(monitor.address, None);
        assert_eq!(monitor.filter_script, "bigint(tx.value) > bigint(1000)");
        assert!(monitor.notifiers.is_empty());
    }
}
