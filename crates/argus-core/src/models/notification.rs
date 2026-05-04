//! Data models for notifications.

use serde::{Deserialize, Serialize};

/// A message to be sent in a notification, with a title and body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct NotificationMessage {
    /// The title of the notification message.
    pub title: String,
    /// The body content of the notification message.
    pub body: String,
}
