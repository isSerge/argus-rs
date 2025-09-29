use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{
    config::{deserialize_duration_from_seconds, serialize_duration_to_seconds},
    models::NotificationMessage,
};

/// Notification policies for handling notifications
#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ActionPolicy {
    /// Policy for aggregating multiple notifications into a single one.
    Aggregation(AggregationPolicy),

    /// Policy for throttling notifications to avoid spamming.
    Throttle(ThrottlePolicy),
}

/// Policy for aggregating multiple notifications into a single one.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct AggregationPolicy {
    /// The time window in seconds for the aggregation policy.
    #[serde(
        deserialize_with = "deserialize_duration_from_seconds",
        serialize_with = "serialize_duration_to_seconds"
    )]
    pub window_secs: Duration,

    /// The template to use for the aggregated notification message.
    pub template: NotificationMessage,
}

/// Policy for throttling notifications to avoid spamming.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ThrottlePolicy {
    /// The maximum number of notifications to send within the specified time
    /// window.
    pub max_count: u32,

    /// The time window in seconds for the throttling policy.
    #[serde(
        deserialize_with = "deserialize_duration_from_seconds",
        serialize_with = "serialize_duration_to_seconds"
    )]
    pub time_window_secs: Duration,
}
