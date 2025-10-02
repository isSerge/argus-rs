use serde::{Deserialize, Serialize};

/// Configuration for NATS publisher.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct NatsConfig {
    /// Comma-separated list of NATS server URLs.
    pub urls: String,

    /// Subject to publish messages to.
    pub subject: String,

    /// Optional credentials for connecting to NATS.
    pub credentials: Option<NatsCredentials>,
}

/// Credentials for NATS authentication.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NatsCredentials {
    /// Optional token for NATS authentication.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    /// Path to a credentials file for NATS authentication.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}
