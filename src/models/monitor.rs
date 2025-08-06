//! This module defines the `Monitor` structure, which represents a blockchain monitor that tracks specific addresses and networks.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

/// Represents a blockchain monitor that tracks specific addresses and networks.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Monitor {
    /// Unique identifier for the monitor (auto-generated when loading from config)
    #[sqlx(rename = "monitor_id")]
    #[serde(default)]
    pub id: i64,
    /// Name of the monitor
    pub name: String,
    /// The blockchain network this monitor is associated with (e.g., Ethereum)
    pub network: String,
    /// The specific address this monitor is tracking
    pub address: String,
    /// The filter script used to determine relevant blockchain events
    pub filter_script: String,
    /// Timestamp when the monitor was created
    #[serde(default = "default_timestamp")]
    pub created_at: DateTime<Utc>,
    /// Timestamp when the monitor was last updated
    #[serde(default = "default_timestamp")]
    pub updated_at: DateTime<Utc>,
}

/// Provides a default timestamp for serde deserialization
fn default_timestamp() -> DateTime<Utc> {
    Utc::now()
}

impl Monitor {
    /// Creates a new monitor from configuration data (without an ID)
    pub fn from_config(
        name: String,
        network: String,
        address: String,
        filter_script: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: 0, // Will be assigned by database
            name,
            network,
            address,
            filter_script,
            created_at: now,
            updated_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monitor_from_config_constructor() {
        let monitor = Monitor::from_config(
            "Test Monitor".to_string(),
            "ethereum".to_string(),
            "0x123".to_string(),
            "log.name == \"Test\"".to_string(),
        );

        assert_eq!(monitor.id, 0);
        assert_eq!(monitor.name, "Test Monitor");
        assert_eq!(monitor.network, "ethereum");
        assert_eq!(monitor.address, "0x123");
        assert_eq!(monitor.filter_script, "log.name == \"Test\"");
    }
}
