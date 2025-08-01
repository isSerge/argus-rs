//! This module defines the `Trigger` structure, which represents a trigger that can be associated with a monitor to perform actions on certain events.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

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
