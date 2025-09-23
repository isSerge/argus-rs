//! This module defines data structures for managing alert-related state.

use argus_models::monitor_match::MonitorMatch;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Represents the current throttling state for a notifier.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThrottleState {
    /// The number of notifications sent within the current throttling window.
    pub count: u32,
    /// The timestamp when the current throttling window started or was last
    /// reset.
    pub window_start_time: DateTime<Utc>,
}

/// Represents the current aggregation state for a notifier.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AggregationState {
    /// The list of monitor matches collected within the current aggregation
    /// window.
    pub matches: Vec<MonitorMatch>,
    /// The timestamp when the current aggregation window started.
    pub window_start_time: DateTime<Utc>,
}
