//! This module defines the `Monitor` structure, which represents a blockchain
//! monitor that tracks specific addresses and networks.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::loader::{Loadable, LoaderError};

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

    /// The name of the ABI for the contract being monitored (e.g., "erc20").
    /// This name references an ABI stored in the database. If `None`, the
    /// monitor will not decode logs.
    #[serde(default, alias = "abi")]
    pub abi_name: Option<String>,

    /// Filter script for the monitor
    pub filter_script: String,

    /// actions for the monitor (optional)
    #[serde(default)]
    pub actions: Vec<String>,

    /// Current status of the monitor
    #[serde(default)]
    pub status: MonitorStatus,
}

impl Loadable for MonitorConfig {
    type Error = LoaderError;

    const KEY: &'static str = "monitors";
}

/// Represents a blockchain monitor retrieved from the database.
#[derive(Debug, Clone, FromRow, Default, Serialize)]
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

    /// The name of the ABI (Application Binary Interface) for the contract
    /// being monitored. This is used to decode event logs and call contract
    /// methods. If `None`, the monitor will not decode logs.
    pub abi_name: Option<String>,

    /// The filter script used to determine relevant blockchain events
    pub filter_script: String,

    /// The actions to execute when the filter script matches
    pub actions: Vec<String>,

    /// Timestamp when the monitor was created
    pub created_at: DateTime<Utc>,

    /// Timestamp when the monitor was last updated
    pub updated_at: DateTime<Utc>,

    /// Current status of the monitor
    #[serde(default)]
    pub status: MonitorStatus,
}

/// Represents the status of a monitor.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "TEXT")]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum MonitorStatus {
    /// The monitor is actively tracking events.
    #[default]
    Active,
    /// The monitor is paused and not tracking events.
    Paused,
}

impl From<String> for MonitorStatus {
    fn from(s: String) -> Self {
        match s.as_str() {
            "paused" => MonitorStatus::Paused,
            _ => MonitorStatus::Active,
        }
    }
}

impl MonitorConfig {
    /// Creates a new monitor from configuration data (without an ID)
    pub fn from_config(
        name: String,
        network: String,
        address: Option<String>,
        abi_name: Option<String>,
        filter_script: String,
        actions: Vec<String>,
    ) -> Self {
        Self {
            name,
            network,
            address,
            abi_name,
            filter_script,
            actions,
            status: MonitorStatus::default(),
        }
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
            Some("test".to_string()),
            "log.name == \"Test\"".to_string(),
            vec!["test-action".to_string()],
        );

        assert_eq!(monitor.name, "Test Monitor");
        assert_eq!(monitor.network, "ethereum");
        assert_eq!(monitor.address, Some("0x123".to_string()));
        assert_eq!(monitor.filter_script, "log.name == \"Test\"");
        assert_eq!(monitor.actions, vec!["test-action".to_string()]);
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
        assert!(monitor.actions.is_empty());
    }
}
