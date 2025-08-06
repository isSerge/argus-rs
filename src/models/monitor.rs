//! This module defines the `Monitor` structure, which represents a blockchain monitor that tracks specific addresses and networks.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

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
}

impl Monitor {
    /// Creates a new monitor from configuration data (without an ID)
    pub fn from_config(name: String, network: String, address: String, filter_script: String) -> Self {
        Self {
            id: 0, // Will be assigned by database
            name,
            network,
            address,
            filter_script,
        }
    }
}
