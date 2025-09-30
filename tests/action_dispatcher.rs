//! Integration tests for the ActionDispatcher service

use std::sync::Arc;

use argus::{
    actions::{ActionDispatcher, ActionPayload},
    config::HttpRetryConfig,
    http_client::HttpClientPool,
    models::{
        NotificationMessage,
        action::{ActionConfig, ActionTypeConfig, DiscordConfig},
        monitor_match::MonitorMatch,
    },
    test_helpers::ActionBuilder,
};
use rdkafka::consumer::Consumer;
use serde_json::json;
use url::Url;

#[tokio::test]
async fn test_webhook_action_success() {
    let mut server = mockito::Server::new_async().await;

    let mock_discord_action = ActionConfig {
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

    let monitor_match = MonitorMatch::new_tx_match(
        1,
        "Test Monitor".to_string(),
        "test_discord".to_string(),
        123,
        Default::default(),
        json!({"value": "100"}),
    );

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

    let monitor_match = MonitorMatch::new_tx_match(
        1,
        "Test Monitor".to_string(),
        "test_stdout".to_string(),
        123,
        Default::default(),
        json!({"value": "100"}),
    );

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

    let monitor_match = MonitorMatch::new_tx_match(
        2,
        "Test Monitor".to_string(),
        "test_discord_retry".to_string(),
        456,
        Default::default(),
        json!({}),
    );

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

    let monitor_match = MonitorMatch::new_tx_match(
        3,
        "Test Monitor".to_string(),
        "test_discord_no_retry".to_string(),
        789,
        Default::default(),
        json!({}),
    );

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

    let monitor_match = MonitorMatch::new_tx_match(
        4,
        "Test Monitor".to_string(),
        "test_invalid_url".to_string(),
        101,
        Default::default(),
        json!({}),
    );

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
    let monitor_match = MonitorMatch::new_tx_match(
        1,
        "Test Monitor".to_string(),
        "test_kafka".to_string(),
        123,
        Default::default(),
        json!({"value": "100"}),
    );
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

    let monitor_match = MonitorMatch::new_tx_match(
        1,
        "Test Monitor".to_string(),
        "test_kafka_failure".to_string(),
        123,
        Default::default(),
        json!({"value": "100"}),
    );

    let payload = ActionPayload::Single(monitor_match.clone());
    let result = action_dispatcher.execute(payload).await;

    assert!(result.is_err());
}
