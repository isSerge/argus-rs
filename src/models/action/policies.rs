use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    config::{deserialize_duration_from_seconds, serialize_duration_to_seconds},
    models::NotificationMessage,
};

#[derive(Debug, Clone, Error)]
pub enum ActionPolicyError {
    #[error("Aggregation window must be greater than zero.")]
    InvalidAggregationWindow,

    #[error("Aggregation template title cannot be empty.")]
    EmptyAggregationTemplateTitle,

    #[error("Throttle max_count must be greater than zero.")]
    InvalidThrottleMaxCount,

    #[error("Throttle time window must be greater than zero.")]
    InvalidThrottleWindow,
}

/// Notification policies for handling notifications
#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ActionPolicy {
    /// Policy for aggregating multiple notifications into a single one.
    Aggregation(AggregationPolicy),

    /// Policy for throttling notifications to avoid spamming.
    Throttle(ThrottlePolicy),
}

impl ActionPolicy {
    /// Validates Action Policy
    pub fn validate(&self) -> Result<(), ActionPolicyError> {
        match self {
            ActionPolicy::Aggregation(policy) => {
                if policy.window_secs <= Duration::ZERO {
                    return Err(ActionPolicyError::InvalidAggregationWindow);
                }
                if policy.template.title.is_empty() {
                    return Err(ActionPolicyError::EmptyAggregationTemplateTitle);
                }
                Ok(())
            }
            ActionPolicy::Throttle(policy) => {
                if policy.time_window_secs <= Duration::ZERO {
                    return Err(ActionPolicyError::InvalidThrottleWindow);
                }
                if policy.max_count <= 0 {
                    return Err(ActionPolicyError::InvalidThrottleMaxCount);
                }
                Ok(())
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn test_validate_aggregation_ok() {
        let policy = ActionPolicy::Aggregation(AggregationPolicy {
            window_secs: Duration::from_secs(60),
            template: NotificationMessage { title: "Test".to_string(), body: "Body".to_string() },
        });
        assert!(policy.validate().is_ok());
    }

    #[test]
    fn test_validate_aggregation_zero_window() {
        let policy = ActionPolicy::Aggregation(AggregationPolicy {
            window_secs: Duration::ZERO,
            template: NotificationMessage { title: "Test".to_string(), body: "Body".to_string() },
        });
        let result = policy.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ActionPolicyError::InvalidAggregationWindow));
    }

    #[test]
    fn test_validate_aggregation_empty_title() {
        let policy = ActionPolicy::Aggregation(AggregationPolicy {
            window_secs: Duration::from_secs(60),
            template: NotificationMessage { title: "".to_string(), body: "Body".to_string() },
        });
        let result = policy.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ActionPolicyError::EmptyAggregationTemplateTitle));
    }

    #[test]
    fn test_validate_throttle_ok() {
        let policy = ActionPolicy::Throttle(ThrottlePolicy {
            max_count: 10,
            time_window_secs: Duration::from_secs(60),
        });
        assert!(policy.validate().is_ok());
    }

    #[test]
    fn test_validate_throttle_zero_max_count() {
        let policy = ActionPolicy::Throttle(ThrottlePolicy {
            max_count: 0,
            time_window_secs: Duration::from_secs(60),
        });
        let result = policy.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ActionPolicyError::InvalidThrottleMaxCount));
    }

    #[test]
    fn test_validate_throttle_zero_window() {
        let policy = ActionPolicy::Throttle(ThrottlePolicy {
            max_count: 10,
            time_window_secs: Duration::ZERO,
        });
        let result = policy.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ActionPolicyError::InvalidThrottleWindow));
    }
}
