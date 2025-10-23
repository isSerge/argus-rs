//! Integration tests for the ActionDispatcher service

mod docker_compose_guard;
use std::sync::Arc;

use argus::{
    action_dispatcher::{ActionDispatcher, ActionPayload, error::ActionDispatcherError},
    config::HttpRetryConfig,
    http_client::HttpClientPool,
    models::{
        NotificationMessage,
        action::{ActionConfig, ActionTypeConfig, DiscordConfig},
        monitor_match::MonitorMatch,
    },
    test_helpers::{ActionBuilder, create_test_monitor_match},
};
use lapin::{
    Connection, ConnectionProperties, ExchangeKind,
    options::{BasicConsumeOptions, ExchangeDeclareOptions, QueueBindOptions, QueueDeclareOptions},
    types::FieldTable,
};
use rdkafka::consumer::Consumer;
use serde_json::json;
use tokio_stream::StreamExt;
use url::Url;

use crate::docker_compose_guard::DockerComposeGuard;

const RABBITMQ_DOCKER_COMPOSE: &str =
    "examples/11_action_with_rabbitmq_publisher/docker-compose.yml";
const NATS_DOCKER_COMPOSE: &str = "examples/12_action_with_nats_publisher/docker-compose.yml";

#[tokio::test]
async fn test_webhook_action_success() {
    let mut server = mockito::Server::new_async().await;

    let mock_discord_action = ActionConfig {
        id: None,
        name: "test_discord".to_string(),
        config: ActionTypeConfig::Discord(DiscordConfig {
            discord_url: Url::parse(&server.url()).unwrap(),
            message: NotificationMessage {
                title: "Test Title".to_string(),
                body: "Monitor {{ monitor_id }} matched on block {{ block_number }} with tx value \
                       {{ tx.value }}"
                    .to_string(),
            },
            retry_policy: Default::default(),
        }),
        policy: None,
    };

    let mock = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"content":"*Test Title*\n\nMonitor 1 matched on block 123 with tx value 100"}"#,
        )
        .create_async()
        .await;

    let http_client_pool = Arc::new(HttpClientPool::default());
    let actions =
        Arc::new(vec![mock_discord_action].into_iter().map(|n| (n.name.clone(), n)).collect());
    let action_dispatcher = ActionDispatcher::new(actions, http_client_pool).await.unwrap();

    let monitor_match = create_test_monitor_match("Test Monitor", "test_discord");

    let payload = ActionPayload::Single(monitor_match.clone());
    let result = action_dispatcher.execute(payload).await;

    assert!(result.is_ok());
    mock.assert();
}

#[tokio::test]
async fn test_stdout_action_success() {
    let stdout_action = ActionBuilder::new("test_stdout").stdout_config(None).build();

    let http_client_pool = Arc::new(HttpClientPool::default());
    let actions = Arc::new(vec![stdout_action].into_iter().map(|n| (n.name.clone(), n)).collect());
    let action_dispatcher = ActionDispatcher::new(actions, http_client_pool).await.unwrap();

    let monitor_match = create_test_monitor_match("Test Monitor", "test_stdout");

    let payload = ActionPayload::Single(monitor_match.clone());
    let result = action_dispatcher.execute(payload).await;

    assert!(result.is_ok(), "Stdout action should succeed, got error: {:?}", result.err());
}

#[tokio::test]
async fn test_failure_with_retryable_error() {
    let mut server = mockito::Server::new_async().await;

    let retry_policy = HttpRetryConfig { max_retry: 2, ..Default::default() };

    let mock_discord_action = ActionBuilder::new("test_discord_retry")
        .discord_config(&server.url())
        .retry_policy(retry_policy.clone())
        .build();

    // Mock a server that fails twice with a retryable error, then succeeds.
    let mock = server
        .mock("POST", "/")
        .with_status(503)
        .with_body("Service Unavailable")
        .expect(3)
        .create_async()
        .await;

    let http_client_pool = Arc::new(HttpClientPool::default());
    let actions =
        Arc::new(vec![mock_discord_action].into_iter().map(|n| (n.name.clone(), n)).collect());
    let action_dispatcher = ActionDispatcher::new(actions, http_client_pool).await.unwrap();

    let monitor_match = MonitorMatch::builder(
        2,
        "Test Monitor".to_string(),
        "test_discord_retry".to_string(),
        456,
        Default::default(),
    )
    .transaction_match(json!({}))
    .decoded_call(None)
    .build();

    let payload = ActionPayload::Single(monitor_match.clone());
    let result = action_dispatcher.execute(payload).await;

    // The final result should be an error because the mock never returns a success
    // status.
    assert!(result.is_err());
    mock.assert();
}

#[tokio::test]
async fn test_failure_with_non_retryable_error() {
    let mut server = mockito::Server::new_async().await;

    let retry_policy = HttpRetryConfig { max_retry: 3, ..Default::default() };

    let mock_discord_action = ActionBuilder::new("test_discord_no_retry")
        .discord_config(&server.url())
        .retry_policy(retry_policy.clone())
        .build();

    // Mock a server that returns a 400 Bad Request, which is a non-retryable error.
    let mock = server
        .mock("POST", "/")
        .with_status(400)
        .with_body("Bad Request")
        .expect(1) // Should only be called once
        .create_async()
        .await;

    let http_client_pool = Arc::new(HttpClientPool::default());
    let actions =
        Arc::new(vec![mock_discord_action].into_iter().map(|n| (n.name.clone(), n)).collect());
    let action_dispatcher = ActionDispatcher::new(actions, http_client_pool).await.unwrap();

    let monitor_match = MonitorMatch::builder(
        3,
        "Test Monitor".to_string(),
        "test_discord_no_retry".to_string(),
        789,
        Default::default(),
    )
    .transaction_match(json!({}))
    .decoded_call(None)
    .build();

    let payload = ActionPayload::Single(monitor_match.clone());
    let result = action_dispatcher.execute(payload).await;

    assert!(result.is_err());
    mock.assert();
}

#[tokio::test]
async fn test_failure_with_invalid_url() {
    let mock_discord_action = ActionBuilder::new("test_invalid_url")
        .discord_config("http://127.0.0.1:1") // Invalid URL that will fail to connect
        .retry_policy(HttpRetryConfig {
            max_retry: 0,
            initial_backoff_ms: std::time::Duration::from_millis(1),
            ..Default::default()
        })
        .build();

    let http_client_pool = Arc::new(HttpClientPool::default());
    let actions =
        Arc::new(vec![mock_discord_action].into_iter().map(|t| (t.name.clone(), t)).collect());
    let action_dispatcher = ActionDispatcher::new(actions, http_client_pool).await.unwrap();

    let monitor_match = MonitorMatch::builder(
        4,
        "Test Monitor".to_string(),
        "test_invalid_url".to_string(),
        101,
        Default::default(),
    )
    .transaction_match(json!({}))
    .decoded_call(None)
    .build();

    let payload = ActionPayload::Single(monitor_match.clone());
    let result = action_dispatcher.execute(payload).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_kafka_action_success() {
    // 1. Set up mock Kafka cluster
    let mock_cluster =
        rdkafka::mocking::MockCluster::new(1).expect("Failed to create mock cluster");
    let topic = "test-topic";
    mock_cluster.create_topic(topic, 1, 1).expect("Failed to create topic");

    // 2. Create a consumer to verify the message
    let consumer: rdkafka::consumer::StreamConsumer = rdkafka::ClientConfig::new()
        .set("bootstrap.servers", &mock_cluster.bootstrap_servers())
        .set("group.id", "test-group")
        .set("auto.offset.reset", "earliest")
        .create()
        .expect("Failed to create consumer");
    consumer.subscribe(&[topic]).expect("Failed to subscribe to topic");

    // 3. Create Kafka action config
    let kafka_action = ActionBuilder::new("test_kafka")
        .kafka_config(&mock_cluster.bootstrap_servers(), topic)
        .build();

    // 4. Create ActionDispatcher
    let http_client_pool = Arc::new(HttpClientPool::default());
    let actions = Arc::new(vec![kafka_action].into_iter().map(|n| (n.name.clone(), n)).collect());
    let action_dispatcher = ActionDispatcher::new(actions, http_client_pool).await.unwrap();

    // 5. Create a monitor match and execute the action
    let monitor_match = MonitorMatch::builder(
        1,
        "Test Monitor".to_string(),
        "test_kafka".to_string(),
        123,
        Default::default(),
    )
    .transaction_match(json!({"value": "100"}))
    .decoded_call(None)
    .build();
    let payload = ActionPayload::Single(monitor_match.clone());
    let result = action_dispatcher.execute(payload.clone()).await;
    assert!(result.is_ok(), "Kafka action should succeed, got error: {:?}", result.err());

    // 6. Verify the message was published
    let message = tokio::time::timeout(std::time::Duration::from_secs(5), consumer.recv())
        .await
        .expect("Timeout waiting for message")
        .expect("Failed to receive message");

    let expected_payload = serde_json::to_vec(&payload.context().unwrap()).unwrap();
    use rdkafka::Message;
    assert_eq!(message.payload(), Some(expected_payload.as_slice()));
    assert_eq!(message.key(), Some(monitor_match.transaction_hash.to_string().as_bytes()));
    assert_eq!(message.topic(), topic);
}

#[tokio::test]
async fn test_kafka_action_failure() {
    let kafka_action = ActionBuilder::new("test_kafka_failure")
        .kafka_config("127.0.0.1:1", "test-topic") // Invalid broker
        .build();

    let http_client_pool = Arc::new(HttpClientPool::default());
    let actions = Arc::new(vec![kafka_action].into_iter().map(|n| (n.name.clone(), n)).collect());
    let action_dispatcher = ActionDispatcher::new(actions, http_client_pool).await.unwrap();

    let monitor_match = MonitorMatch::builder(
        1,
        "Test Monitor".to_string(),
        "test_kafka_failure".to_string(),
        123,
        Default::default(),
    )
    .transaction_match(json!({"value": "100"}))
    .decoded_call(None)
    .build();

    let payload = ActionPayload::Single(monitor_match.clone());
    let result = action_dispatcher.execute(payload).await;

    assert!(result.is_err());
}

// Tests for RabbitMQ action using a real RabbitMQ instance in Docker
#[tokio::test]
#[ignore]
async fn test_rabbitmq_action_success() {
    let _docker_guard = DockerComposeGuard::new(RABBITMQ_DOCKER_COMPOSE);
    // 1. Set up RabbitMQ connection URI
    let rabbitmq_uri = "amqp://guest:guest@localhost:5672/%2f";
    let exchange = &format!("test-exchange-{}", std::process::id()); // Unique exchange name using process ID
    let routing_key = "test-key";

    // 2. Create a consumer to verify the message
    let conn = Connection::connect(rabbitmq_uri, ConnectionProperties::default())
        .await
        .expect("Failed to connect to RabbitMQ");
    let channel = conn.create_channel().await.expect("Failed to create channel");

    // Ensure the exchange exists before binding a queue to it
    channel
        .exchange_declare(
            exchange,
            ExchangeKind::Topic,
            ExchangeDeclareOptions { durable: true, ..Default::default() },
            FieldTable::default(),
        )
        .await
        .expect("Failed to declare exchange");

    let queue = channel
        .queue_declare("", QueueDeclareOptions::default(), FieldTable::default())
        .await
        .expect("Failed to declare queue");
    channel
        .queue_bind(
            queue.name().as_str(),
            exchange,
            routing_key,
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await
        .expect("Failed to bind queue");
    let mut consumer = channel
        .basic_consume(
            queue.name().as_str(),
            "test-consumer",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await
        .expect("Failed to create consumer");

    // 3. Create RabbitMQ action config
    let rabbitmq_action = ActionBuilder::new("test_rabbitmq")
        .rabbitmq_config(rabbitmq_uri, exchange, routing_key)
        .build();

    // 4. Create ActionDispatcher
    let http_client_pool = Arc::new(HttpClientPool::default());
    let actions =
        Arc::new(vec![rabbitmq_action].into_iter().map(|n| (n.name.clone(), n)).collect());
    let action_dispatcher = ActionDispatcher::new(actions, http_client_pool).await.unwrap();

    // 5. Create a monitor match and execute the action
    let monitor_match = MonitorMatch::builder(
        1,
        "Test Monitor".to_string(),
        "test_rabbitmq".to_string(),
        123,
        Default::default(),
    )
    .transaction_match(json!({"value": "100"}))
    .decoded_call(None)
    .build();
    let payload = ActionPayload::Single(monitor_match.clone());
    let result = action_dispatcher.execute(payload.clone()).await;
    assert!(result.is_ok(), "RabbitMQ action should succeed, got error: {:?}", result.err());

    // 6. Verify the message was published
    let delivery = tokio::time::timeout(std::time::Duration::from_secs(5), consumer.next())
        .await
        .expect("Timeout waiting for message")
        .expect("Failed to receive message")
        .expect("Error in received message");

    let expected_payload = serde_json::to_vec(&payload.context().unwrap()).unwrap();
    assert_eq!(delivery.data, expected_payload);
    assert_eq!(delivery.exchange.as_str(), exchange);
    assert_eq!(delivery.routing_key.as_str(), routing_key);
}

#[tokio::test]
async fn test_rabbitmq_action_failure() {
    let rabbitmq_action = ActionBuilder::new("test_rabbitmq_failure")
        .rabbitmq_config("amqp://guest:guest@127.0.0.1:1/%2f", "test-exchange", "test-key") // Invalid port
        .build();

    let http_client_pool = Arc::new(HttpClientPool::default());
    let actions =
        Arc::new(vec![rabbitmq_action].into_iter().map(|n| (n.name.clone(), n)).collect());
    let action_dispatcher = ActionDispatcher::new(actions, http_client_pool).await.unwrap();

    let monitor_match = MonitorMatch::builder(
        1,
        "Test Monitor".to_string(),
        "test_rabbitmq_failure".to_string(),
        123,
        Default::default(),
    )
    .transaction_match(json!({"value": "100"}))
    .decoded_call(None)
    .build();

    let payload = ActionPayload::Single(monitor_match.clone());
    let result = action_dispatcher.execute(payload).await;

    assert!(result.is_err());
    assert!(matches!(result.err().unwrap(), ActionDispatcherError::ConfigError(_)));
}

#[tokio::test]
#[ignore]
async fn test_nats_action_success() {
    let _docker_guard = DockerComposeGuard::new(NATS_DOCKER_COMPOSE);
    // 1. Set up NATS connection URI
    let nats_uri = "nats://localhost:4222";
    let subject = &format!("test-subject-{}", std::process::id()); // Unique subject name using process ID

    // 2. Create a subscriber to verify the message
    let client = async_nats::connect(nats_uri).await.expect("Failed to connect to NATS");
    let mut subscriber = client.subscribe(subject.to_string()).await.expect("Failed to subscribe");

    // 3. Create NATS action config
    let nats_action = ActionBuilder::new("test_nats").nats_config(nats_uri, subject).build();

    // 4. Create ActionDispatcher
    let http_client_pool = Arc::new(HttpClientPool::default());
    let actions = Arc::new(vec![nats_action].into_iter().map(|n| (n.name.clone(), n)).collect());
    let action_dispatcher = ActionDispatcher::new(actions, http_client_pool).await.unwrap();

    // 5. Create a monitor match and execute the action
    let monitor_match = MonitorMatch::builder(
        1,
        "Test Monitor".to_string(),
        "test_nats".to_string(),
        123,
        Default::default(),
    )
    .transaction_match(json!({"value": "100"}))
    .decoded_call(None)
    .build();
    let payload = ActionPayload::Single(monitor_match.clone());
    let result = action_dispatcher.execute(payload.clone()).await;
    assert!(result.is_ok(), "NATS action should succeed, got error: {:?}", result.err());

    // 6. Verify the message was published
    let message = tokio::time::timeout(std::time::Duration::from_secs(5), subscriber.next())
        .await
        .expect("Timeout waiting for message")
        .expect("Failed to receive message");

    let expected_payload = serde_json::to_vec(&payload.context().unwrap()).unwrap();
    assert_eq!(message.payload.to_vec(), expected_payload);
    assert_eq!(message.subject.as_str(), subject);
}

#[tokio::test]
async fn test_nats_action_failure() {
    let nats_action = ActionBuilder::new("test_nats_failure")
        .nats_config("nats://127.0.0.1:1", "test-subject") // Invalid port
        .build();

    let http_client_pool = Arc::new(HttpClientPool::default());
    let actions = Arc::new(vec![nats_action].into_iter().map(|n| (n.name.clone(), n)).collect());
    let action_dispatcher = ActionDispatcher::new(actions, http_client_pool).await.unwrap();

    let monitor_match = MonitorMatch::builder(
        1,
        "Test Monitor".to_string(),
        "test_nats_failure".to_string(),
        123,
        Default::default(),
    )
    .transaction_match(json!({"value": "100"}))
    .decoded_call(None)
    .build();

    let payload = ActionPayload::Single(monitor_match.clone());
    let result = action_dispatcher.execute(payload).await;

    assert!(result.is_err());
    assert!(matches!(result.err().unwrap(), ActionDispatcherError::ConfigError(_)));
}
