use serde::{Deserialize, Serialize};

/// Configuration for a Kafka event publisher.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct KafkaConfig {
    /// The Kafka topic to publish messages to.
    pub topic: String,

    /// Comma-separated list of Kafka broker addresses.
    pub brokers: String,

    /// Optional security configuration for connecting to Kafka.
    #[serde(default)]
    pub security: KafkaSecurityConfig,

    /// Optional producer-specific configuration properties.
    #[serde(default)]
    pub producer: KafkaProducerConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct KafkaSecurityConfig {
    /// The security protocol to use. Common values are PLAINTEXT, SSL,
    /// SASL_PLAINTEXT, SASL_SSL.
    pub protocol: String,

    /// The SASL mechanism to use for authentication.
    /// Common values: PLAIN, SCRAM-SHA-256, SCRAM-SHA-512.
    /// Only used if protocol is SASL_PLAINTEXT or SASL_SSL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sasl_mechanism: Option<String>,

    /// The username for SASL authentication. Can use environment variable
    // expansion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sasl_username: Option<String>,

    /// The password for SASL authentication. Can use environment variable
    // expansion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sasl_password: Option<String>,

    /// Path to the CA certificate file for verifying the broker's certificate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssl_ca_location: Option<String>,
}

impl Default for KafkaSecurityConfig {
    fn default() -> Self {
        Self {
            protocol: "PLAINTEXT".to_string(),
            sasl_mechanism: None,
            sasl_username: None,
            sasl_password: None,
            ssl_ca_location: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct KafkaProducerConfig {
    // The maximum time in milliseconds to wait for a message to be sent.
    /// librdkafka property: `message.timeout.ms`
    #[serde(default = "default_message_timeout_ms")]
    pub message_timeout_ms: u64,

    /// The compression codec to use for compressing message sets.
    /// Common values: none, gzip, snappy, lz4, zstd.
    /// librdkafka property: `compression.codec`
    #[serde(default = "default_compression_codec")]
    pub compression_codec: String,

    /// The number of acknowledgments the producer requires the leader to have
    /// received before considering a request complete.
    /// `0`: Producer does not wait for any acknowledgment.
    /// `1`: The leader will write the record to its local log but will respond
    // without awaiting full acknowledgment from all followers.
    /// `all` or `-1`: The leader will wait for the full set of in-sync replicas
    // to acknowledge the record.
    /// librdkafka property: `acks`
    #[serde(default = "default_acks")]
    pub acks: String,
}

// Default functions for KafkaProducerConfig
fn default_message_timeout_ms() -> u64 {
    5000
}
fn default_compression_codec() -> String {
    "none".to_string()
}
fn default_acks() -> String {
    "all".to_string()
}

impl Default for KafkaProducerConfig {
    fn default() -> Self {
        Self {
            message_timeout_ms: default_message_timeout_ms(),
            compression_codec: default_compression_codec(),
            acks: default_acks(),
        }
    }
}
