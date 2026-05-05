/// Error types for event publishers.
#[derive(Debug, thiserror::Error)]
pub enum PublisherError {
    /// Kafka error
    #[cfg(feature = "kafka")]
    #[error("Kafka error: {0}")]
    Kafka(#[from] rdkafka::error::KafkaError),

    /// RabbitMQ error
    #[cfg(feature = "rabbitmq")]
    #[error("RabbitMQ error: {0}")]
    Lapin(#[from] lapin::Error),

    /// NATS IO error for credentials loading.
    #[cfg(feature = "nats")]
    #[error("NATS credentials error: {0}")]
    NatsIo(#[from] std::io::Error),

    /// NATS connection error.
    #[cfg(feature = "nats")]
    #[error("NATS connection error: {0}")]
    NatsConnect(#[from] async_nats::ConnectError),

    /// NATS publish error.
    #[cfg(feature = "nats")]
    #[error("NATS publish error: {0}")]
    NatsPublish(#[from] async_nats::PublishError),

    /// NATS flush error.
    #[cfg(feature = "nats")]
    #[error("NATS flush error: {0}")]
    NatsFlush(#[from] async_nats::client::FlushError),
}
