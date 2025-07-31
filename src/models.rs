//! This module contains the data models for the Argus application.

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

/// Represents a trigger that can be associated with a monitor to perform actions on certain events.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Trigger {
    /// Unique identifier for the trigger
    #[sqlx(rename = "trigger_id")]
    pub id: i64,
    /// The unique identifier for the monitor this trigger is associated with
    pub monitor_id: i64,
    /// Trigger details (placeholder)
    pub details: String,
}
