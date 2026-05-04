use serde::{Deserialize, Serialize};

use crate::models::NotificationMessage;

/// Configuration for a Stdout notification.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
pub struct StdoutConfig {
    /// The optional message content for the notification.
    /// If not provided, the full event payload will be serialized to JSON.
    pub message: Option<NotificationMessage>,
}
