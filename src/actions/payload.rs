use crate::{
    actions::error::ActionDispatcherError,
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
}
