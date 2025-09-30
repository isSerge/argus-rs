use std::time::Duration;

use rdkafka::{
    ClientConfig,
    producer::{FutureProducer, FutureRecord, Producer},
};

use crate::{
    actions::publisher::{EventPublisher, PublisherError},
    models::action::KafkaConfig,
};

/// A Kafka event publisher.
#[derive(Clone)]
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
            .map_err(|(kafka_error, _)| PublisherError::Kafka(kafka_error))?;

        Ok(())
    }

    async fn flush(&self, timeout: Duration) -> Result<(), PublisherError> {
        self.producer.flush(timeout).map_err(|e| e.into())
    }
}

impl KafkaEventPublisher {
    /// Creates a new `KafkaEventPublisher` from the given `FutureProducer`.
    pub fn new(producer: FutureProducer) -> Self {
        Self { producer }
    }

    /// Creates a new `KafkaEventPublisher` from the given `KafkaConfig`.
    pub fn from_config(config: &KafkaConfig) -> Result<Self, PublisherError> {
        let mut client_config = ClientConfig::new();

        // Set list of brokers
        client_config.set("bootstrap.servers", &config.brokers);

        // Set security protocol settings
        client_config.set("security.protocol", &config.security.protocol);

        if config.security.protocol.starts_with("SASL") {
            if let Some(mechanism) = &config.security.sasl_mechanism {
                client_config.set("sasl.mechanism", mechanism);
            }
            if let Some(username) = &config.security.sasl_username {
                client_config.set("sasl.username", username);
            }
            if let Some(password) = &config.security.sasl_password {
                client_config.set("sasl.password", password);
            }
        }

        if config.security.protocol.ends_with("SSL")
            && let Some(ca_location) = &config.security.ssl_ca_location
        {
            client_config.set("ssl.ca.location", ca_location);
        }

        // Set producer-specific settings
        client_config.set("acks", &config.producer.acks);
        client_config.set("message.timeout.ms", config.producer.message_timeout_ms.to_string());
        client_config.set("compression.codec", &config.producer.compression_codec);

        // Create the FutureProducer
        let producer = client_config.create::<FutureProducer>()?;

        Ok(Self::new(producer))
    }
}

#[cfg(test)]
mod tests {
    use rdkafka::{
        Message,
        consumer::{Consumer, StreamConsumer},
        mocking::MockCluster,
    };

    use super::*;
    use crate::models::action::{KafkaProducerConfig, KafkaSecurityConfig};

    #[test]
    fn test_kafka_event_publisher_from_config_default() {
        let config = KafkaConfig {
            brokers: "localhost:9092".to_string(),
            topic: "test-topic".to_string(),
            ..Default::default()
        };

        let publisher = KafkaEventPublisher::from_config(&config);
        assert!(publisher.is_ok());
    }

    #[test]
    fn test_kafka_event_publisher_from_config_with_security_settings() {
        let config = KafkaConfig {
            brokers: "localhost:9092".to_string(),
            topic: "test-topic".to_string(),
            security: KafkaSecurityConfig {
                protocol: "SASL_PLAINTEXT".to_string(),
                sasl_mechanism: Some("PLAIN".to_string()),
                sasl_username: Some("user".to_string()),
                sasl_password: Some("password".to_string()),
                ssl_ca_location: Some("/path/to/ca.pem".to_string()),
            },
            ..Default::default()
        };

        let publisher = KafkaEventPublisher::from_config(&config);
        assert!(publisher.is_ok(), "Expected Ok, got error: {:?}", publisher.err());
    }

    #[test]
    fn test_kafka_event_publisher_from_config_with_producer_settings() {
        let config = KafkaConfig {
            brokers: "localhost:9092".to_string(),
            topic: "test-topic".to_string(),
            producer: KafkaProducerConfig {
                message_timeout_ms: 10000,
                acks: "all".to_string(),
                compression_codec: "gzip".to_string(),
            },
            ..Default::default()
        };

        let publisher = KafkaEventPublisher::from_config(&config);
        assert!(publisher.is_ok());
    }

    #[tokio::test]
    async fn test_kafka_event_publisher_publish() {
        let mock_cluster = MockCluster::new(1).expect("Failed to create mock cluster");
        let topic = "test-topic";

        mock_cluster.create_topic(topic, 1, 1).expect("Failed to create topic");

        let consumer = ClientConfig::new()
            .set("bootstrap.servers", &mock_cluster.bootstrap_servers())
            .set("group.id", "test-group")
            .set("auto.offset.reset", "earliest")
            .create::<StreamConsumer>()
            .expect("Failed to create consumer");

        consumer.subscribe(&[topic]).expect("Failed to subscribe to topic");

        let config = KafkaConfig {
            brokers: mock_cluster.bootstrap_servers(),
            topic: topic.to_string(),
            ..Default::default()
        };

        let publisher =
            KafkaEventPublisher::from_config(&config).expect("Failed to create publisher");

        let key = "test-key";
        let payload = b"test-payload";

        let result = publisher.publish(&config.topic, key, payload).await;
        assert!(result.is_ok());

        let message = consumer.recv().await.expect("Failed to receive message");
        assert_eq!(message.key(), Some(key.as_bytes()));
        assert_eq!(message.payload(), Some(payload.as_ref()));
        assert_eq!(message.topic(), topic.to_string());
    }

    #[tokio::test]
    async fn test_kafka_event_publisher_flush() {
        let mock_cluster = MockCluster::new(1).expect("Failed to create mock cluster");
        let topic = "test-topic";

        mock_cluster.create_topic(topic, 1, 1).expect("Failed to create topic");

        let consumer = ClientConfig::new()
            .set("bootstrap.servers", &mock_cluster.bootstrap_servers())
            .set("group.id", "test-group")
            .set("auto.offset.reset", "earliest")
            .create::<StreamConsumer>()
            .expect("Failed to create consumer");

        consumer.subscribe(&[topic]).expect("Failed to subscribe to topic");

        let config = KafkaConfig {
            brokers: mock_cluster.bootstrap_servers(),
            topic: topic.to_string(),
            ..Default::default()
        };

        let publisher =
            KafkaEventPublisher::from_config(&config).expect("Failed to create publisher");

        let key = "flush-key";
        let payload = b"test-payload";

        let publisher_clone = publisher.clone();

        tokio::spawn(async move {
            let result = publisher_clone.publish(&config.topic, key, payload).await;
            assert!(result.is_ok());
        });

        tokio::time::sleep(Duration::from_millis(100)).await;

        let result = publisher.flush(Duration::from_secs(5)).await;
        assert!(result.is_ok());

        let message = consumer.recv().await.expect("Failed to receive message");

        assert_eq!(message.key(), Some(key.as_bytes()));
        assert_eq!(message.payload(), Some(payload.as_ref()));
        assert_eq!(message.topic(), topic.to_string());
    }
}
