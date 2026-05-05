/// Error types for event publishers.
#[derive(Debug, thiserror::Error)]
pub enum PublisherError {
    /// Kafka error
    #[error("Kafka error: {0}")]
    Kafka(#[from] rdkafka::error::KafkaError),

    /// RabbitMQ error
    #[error("RabbitMQ error: {0}")]
    Lapin(#[from] lapin::Error),

    /// NATS IO error for credentials loading.
    #[error("NATS credentials error: {0}")]
    NatsIo(#[from] std::io::Error),

    /// NATS connection error.
    #[error("NATS connection error: {0}")]
    NatsConnect(#[from] async_nats::ConnectError),

    /// NATS publish error.
    #[error("NATS publish error: {0}")]
    NatsPublish(#[from] async_nats::PublishError),
}
