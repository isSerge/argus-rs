//! This module defines the `Monitor` structure, which represents a blockchain monitor that tracks specific addresses and networks.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Represents a blockchain monitor that tracks specific addresses and networks.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Monitor {
    /// Unique identifier for the monitor
    #[sqlx(rename = "monitor_id")]
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
