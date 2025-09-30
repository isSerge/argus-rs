#[derive(Debug, thiserror::Error)]
pub enum PublisherError {
    #[error("Kafka error: {0}")]
    KafkaError(#[from] rdkafka::error::KafkaError),
}
