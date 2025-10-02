/// Error types for event publishers.
#[derive(Debug, thiserror::Error)]
pub enum PublisherError {
    /// Kafka error
    #[error("Kafka error: {0}")]
    Kafka(#[from] rdkafka::error::KafkaError),

    /// RabbitMQ error
    #[error("RabbitMQ error: {0}")]
    Lapin(#[from] lapin::Error),
}
