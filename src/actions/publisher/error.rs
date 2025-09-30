#[derive(Debug, thiserror::Error)]
pub enum PublisherError {
    #[error("Kafka error: {0}")]
    Kafka(#[from] rdkafka::error::KafkaError),

    #[error("RabbitMQ error: {0}")]
    Lapin(#[from] lapin::Error),
}
