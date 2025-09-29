use std::time::Duration;

use rdkafka::producer::{FutureProducer, FutureRecord, Producer};

use crate::actions::publisher::{EventPublisher, PublisherError};

/// A Kafka event publisher.
pub struct KafkaEventPublisher {
    producer: FutureProducer,
}

#[async_trait::async_trait]
impl EventPublisher for KafkaEventPublisher {
    async fn publish(&self, topic: &str, key: &str, payload: &[u8]) -> Result<(), PublisherError> {
        let record = FutureRecord::to(topic).key(key).payload(payload);

        self.producer
            .send(record, Duration::from_secs(0))
            .await
            .map(|_| ())
            .map_err(|(kafka_error, _)| PublisherError::KafkaError(kafka_error))?;

        Ok(())
    }

    async fn flush(&self, timeout: Duration) -> Result<(), PublisherError> {
        self.producer.flush(timeout).map_err(|e| e.into())
    }
}
