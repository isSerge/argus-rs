use crate::{
    action_dispatcher::error::ActionDispatcherError,
    models::{NotificationMessage, monitor_match::MonitorMatch},
};

/// An enum representing the different types of action payloads.
#[derive(Clone)]
pub enum ActionPayload {
    /// A single monitor match.
    Single(MonitorMatch),
    /// An aggregated item for multiple monitor matches.
    Aggregated {
        /// The name of the action to use for this aggregated item.
        action_name: String,
        /// The list of monitor matches to include in the aggregation.
        matches: Vec<MonitorMatch>,
        /// The template to use for the notification message.
        template: NotificationMessage,
    },
}

impl ActionPayload {
    /// Serializes the payload context to a JSON value.
    pub fn context(&self) -> Result<serde_json::Value, ActionDispatcherError> {
        match self {
            ActionPayload::Single(monitor_match) =>
                serde_json::to_value(monitor_match).map_err(|e| {
                    ActionDispatcherError::InternalError(format!(
                        "Failed to serialize monitor match: {e}"
                    ))
                }),
            ActionPayload::Aggregated { matches, .. } => {
                let monitor_name = matches.first().map(|m| m.monitor_name.clone());
                let context = serde_json::json!({
                    "matches": matches,
                    "monitor_name": monitor_name,
                });
                Ok(context)
            }
        }
    }

    /// Returns the name of the action associated with this payload.
    pub fn action_name(&self) -> String {
        match self {
            ActionPayload::Single(monitor_match) => monitor_match.action_name.clone(),
            ActionPayload::Aggregated { action_name, .. } => action_name.clone(),
        }
    }

    /// Returns the monitor name associated with this payload, if available.
    pub fn monitor_name(&self) -> String {
        match self {
            ActionPayload::Single(monitor_match) => monitor_match.monitor_name.clone(),
            ActionPayload::Aggregated { matches, .. } =>
                matches.first().map(|m| m.monitor_name.clone()).unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::TxHash;

    use super::*;

    #[test]
    fn test_single_payload_context() {
        let monitor_match = MonitorMatch::builder(
            1,
            "test-monitor".to_string(),
            "test-action".to_string(),
            123,
            TxHash::default(),
        )
        .transaction_match(serde_json::json!({ "foo": "bar" }))
        .decoded_call(None)
        .build();
        let payload = ActionPayload::Single(monitor_match.clone());
        let context = payload.context().unwrap();
        let expected_context = serde_json::to_value(&monitor_match).unwrap();
        assert_eq!(context, expected_context);
    }

    #[test]
    fn test_aggregated_payload_context() {
        let monitor_match1 = MonitorMatch::builder(
            1,
            "test-monitor".to_string(),
            "test-action".to_string(),
            123,
            TxHash::default(),
        )
        .transaction_match(serde_json::json!({ "foo": "bar" }))
        .decoded_call(None)
        .build();
        let monitor_match2 = MonitorMatch::builder(
            2,
            "test-monitor".to_string(),
            "test-action".to_string(),
            124,
            TxHash::default(),
        )
        .transaction_match(serde_json::json!({ "baz": "qux" }))
        .decoded_call(None)
        .build();
        let payload = ActionPayload::Aggregated {
            action_name: "test-action".to_string(),
            matches: vec![monitor_match1.clone(), monitor_match2.clone()],
            template: NotificationMessage {
                title: "Test Title".to_string(),
                body: "Test Body".to_string(),
            },
        };
        let context = payload.context().unwrap();
        let expected_context = serde_json::json!({
            "matches": [monitor_match1, monitor_match2],
            "monitor_name": "test-monitor",
        });
        assert_eq!(context, expected_context);
    }

    #[test]
    fn test_action_name() {
        let monitor_match = MonitorMatch::builder(
            1,
            "test-monitor".to_string(),
            "test-action".to_string(),
            123,
            TxHash::default(),
        )
        .transaction_match(serde_json::json!({ "foo": "bar" }))
        .decoded_call(None)
        .build();
        let single_payload = ActionPayload::Single(monitor_match.clone());
        assert_eq!(single_payload.action_name(), "test-action");

        let aggregated_payload = ActionPayload::Aggregated {
            action_name: "aggregated-action".to_string(),
            matches: vec![monitor_match],
            template: NotificationMessage {
                title: "Test Title".to_string(),
                body: "Test Body".to_string(),
            },
        };
        assert_eq!(aggregated_payload.action_name(), "aggregated-action");
    }

    #[test]
    fn test_monitor_name() {
        let monitor_match = MonitorMatch::builder(
            1,
            "test-monitor".to_string(),
            "test-action".to_string(),
            123,
            TxHash::default(),
        )
        .transaction_match(serde_json::json!({ "foo": "bar" }))
        .decoded_call(None)
        .build();
        let single_payload = ActionPayload::Single(monitor_match.clone());
        assert_eq!(single_payload.monitor_name(), "test-monitor");

        let aggregated_payload = ActionPayload::Aggregated {
            action_name: "aggregated-action".to_string(),
            matches: vec![monitor_match],
            template: NotificationMessage {
                title: "Test Title".to_string(),
                body: "Test Body".to_string(),
            },
        };
        assert_eq!(aggregated_payload.monitor_name(), "test-monitor");
    }
}
