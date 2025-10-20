//! Integration tests for event publishers (Kafka, RabbitMQ).
//!
//! These tests are ignored by default and should only be run in a CI
//! environment with Docker available. They use the docker-compose files from
//! the examples to spin up the necessary services.
//!
//! To run these tests locally:
//! `cargo test -- --ignored`

mod docker_compose_guard;
use std::time::Duration;

use alloy::primitives::TxHash;
use argus::{
    actions::{
        ActionPayload,
        publisher::{
            EventPublisher, KafkaEventPublisher, NatsEventPublisher, RabbitMqEventPublisher,
        },
    },
    models::{
        action::{KafkaConfig, NatsConfig, RabbitMqConfig},
        monitor_match::MonitorMatch,
    },
};
use rdkafka::{
    ClientConfig, Message,
    consumer::{Consumer, StreamConsumer},
};
use tokio::time::timeout;
use tokio_stream::StreamExt;

use crate::docker_compose_guard::DockerComposeGuard;

const KAFKA_DOCKER_COMPOSE: &str = "examples/10_action_with_kafka_publisher/docker-compose.yml";
const RABBITMQ_DOCKER_COMPOSE: &str =
    "examples/11_action_with_rabbitmq_publisher/docker-compose.yml";
const NATS_DOCKER_COMPOSE: &str = "examples/12_action_with_nats_publisher/docker-compose.yml";

fn create_test_payload() -> ActionPayload {
    let monitor_match = MonitorMatch::new_tx_match(
        1,
        "test-monitor".to_string(),
        "test-action".to_string(),
        123,
        TxHash::default(),
        serde_json::json!({ "foo": "bar" }),
        None,
    );
    ActionPayload::Single(monitor_match)
}

#[tokio::test]
#[ignore]
async fn test_kafka_publisher_integration() {
    let _docker_guard = DockerComposeGuard::new(KAFKA_DOCKER_COMPOSE);

    let kafka_config = KafkaConfig {
        brokers: "127.0.0.1:9092".to_string(),
        topic: "argus-integration-test".to_string(),
        ..Default::default()
    };

    let publisher = KafkaEventPublisher::from_config(&kafka_config).unwrap();

    let payload = create_test_payload();
    let serialized_payload = payload.context().unwrap().to_string();
    let key = payload.action_name();

    publisher.publish(&kafka_config.topic, &key, serialized_payload.as_bytes()).await.unwrap();

    // Verify the message was sent
    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", &kafka_config.brokers)
        .set("group.id", "argus-integration-test-group")
        .set("auto.offset.reset", "earliest")
        .create()
        .expect("Consumer creation failed");

    consumer.subscribe(&[&kafka_config.topic]).expect("Can't subscribe to topic");

    let message_result = timeout(Duration::from_secs(10), consumer.recv()).await;
    assert!(message_result.is_ok(), "Timed out waiting for message from Kafka");

    let message = message_result.unwrap().expect("Error receiving message");
    let received_payload = message.payload().expect("Message has no payload");

    assert_eq!(received_payload, serialized_payload.as_bytes());
}

#[tokio::test]
#[ignore]
async fn test_rabbitmq_publisher_integration() {
    let _docker_guard = DockerComposeGuard::new(RABBITMQ_DOCKER_COMPOSE);

    let rabbitmq_config = RabbitMqConfig {
        uri: "amqp://guest:guest@127.0.0.1:5672/%2f".to_string(),
        exchange: "argus-integration-exchange".to_string(),
        exchange_type: "topic".to_string(),
        routing_key: Some("integration.test".to_string()),
    };

    let publisher = RabbitMqEventPublisher::from_config(&rabbitmq_config).await.unwrap();

    // Setup a queue to consume the message
    let conn =
        lapin::Connection::connect(&rabbitmq_config.uri, lapin::ConnectionProperties::default())
            .await
            .unwrap();
    let channel = conn.create_channel().await.unwrap();
    let queue_name = "integration-test-queue";
    let _queue = channel
        .queue_declare(
            queue_name,
            lapin::options::QueueDeclareOptions::default(),
            lapin::types::FieldTable::default(),
        )
        .await
        .unwrap();
    channel
        .queue_bind(
            queue_name,
            &rabbitmq_config.exchange,
            rabbitmq_config.routing_key.as_deref().unwrap(),
            lapin::options::QueueBindOptions::default(),
            lapin::types::FieldTable::default(),
        )
        .await
        .unwrap();

    let payload = create_test_payload();
    let serialized_payload = payload.context().unwrap().to_string();
    let key = payload.action_name();

    publisher
        .publish(
            rabbitmq_config.routing_key.as_deref().unwrap(),
            &key,
            serialized_payload.as_bytes(),
        )
        .await
        .unwrap();

    // Verify the message was sent
    let mut consumer = channel
        .basic_consume(
            queue_name,
            "test-consumer",
            lapin::options::BasicConsumeOptions::default(),
            lapin::types::FieldTable::default(),
        )
        .await
        .unwrap();

    let delivery_result = timeout(Duration::from_secs(10), consumer.next()).await;
    assert!(delivery_result.is_ok(), "Timed out waiting for message from RabbitMQ");

    let delivery = delivery_result.unwrap().unwrap().expect("Error receiving message");
    assert_eq!(delivery.data, serialized_payload.as_bytes());
}

#[tokio::test]
#[ignore]
async fn test_nats_publisher_integration() {
    let _docker_guard = DockerComposeGuard::new(NATS_DOCKER_COMPOSE);

    let nats_config = NatsConfig {
        urls: "nats://127.0.0.1:4222".to_string(),
        subject: "argus-integration-test".to_string(),
        credentials: None,
    };

    let publisher = NatsEventPublisher::from_config(&nats_config).await.unwrap();

    // Setup a subscriber to consume the message
    let client = async_nats::connect(&nats_config.urls).await.unwrap();
    let mut subscriber = client.subscribe(nats_config.subject.clone()).await.unwrap();

    let payload = create_test_payload();
    let serialized_payload = payload.context().unwrap().to_string();
    let key = payload.action_name();

    publisher.publish(&nats_config.subject, &key, serialized_payload.as_bytes()).await.unwrap();

    // Verify the message was sent
    let message_result = timeout(Duration::from_secs(10), subscriber.next()).await;
    assert!(message_result.is_ok(), "Timed out waiting for message from NATS");

    let message = message_result.unwrap().unwrap();
    assert_eq!(message.payload.to_vec(), serialized_payload.as_bytes());
}
