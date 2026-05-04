use serde::{Deserialize, Serialize};

/// Configuration for a RabbitMQ event publisher.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct RabbitMqConfig {
    /// The URI for connecting to the RabbitMQ server.
    pub uri: String,

    /// The RabbitMQ exchange to publish messages to.
    pub exchange: String,

    /// Optional exchange type (e.g., "direct", "topic", "fanout").
    #[serde(default = "default_exchange_type")]
    pub exchange_type: String,

    /// Optional routing key for message routing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing_key: Option<String>,
}

fn default_exchange_type() -> String {
    "topic".to_string()
}
