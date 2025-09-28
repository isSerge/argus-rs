//! Integration tests for the notification service

use std::sync::Arc;

use argus::{
    config::HttpRetryConfig,
    http_client::HttpClientPool,
    models::{
        NotificationMessage,
        monitor_match::MonitorMatch,
        notifier::{DiscordConfig, NotifierConfig, NotifierTypeConfig},
    },
    notification::{NotificationPayload, NotificationService},
    test_helpers::NotifierBuilder,
};
use mockito;
use serde_json::json;
use url::Url;

#[tokio::test]
async fn test_success() {
    let mut server = mockito::Server::new_async().await;

    let mock_discord_notifier = NotifierConfig {
        name: "test_discord".to_string(),
        config: NotifierTypeConfig::Discord(DiscordConfig {
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
    let notifiers =
        Arc::new(vec![mock_discord_notifier].into_iter().map(|n| (n.name.clone(), n)).collect());
    let notification_service = NotificationService::new(notifiers, http_client_pool);

    let monitor_match = MonitorMatch::new_tx_match(
        1,
        "Test Monitor".to_string(),
        "test_discord".to_string(),
        123,
        Default::default(),
        json!({"value": "100"}),
    );

    let payload = NotificationPayload::Single(monitor_match.clone());
    let result = notification_service.execute(payload).await;

    assert!(result.is_ok());
    mock.assert();
}

#[tokio::test]
async fn test_failure_with_retryable_error() {
    let mut server = mockito::Server::new_async().await;

    let retry_policy = HttpRetryConfig { max_retry: 2, ..Default::default() };

    let mock_discord_notifier = NotifierBuilder::new("test_discord_retry")
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
    let notifiers =
        Arc::new(vec![mock_discord_notifier].into_iter().map(|n| (n.name.clone(), n)).collect());
    let notification_service = NotificationService::new(notifiers, http_client_pool);

    let monitor_match = MonitorMatch::new_tx_match(
        2,
        "Test Monitor".to_string(),
        "test_discord_retry".to_string(),
        456,
        Default::default(),
        json!({}),
    );

    let payload = NotificationPayload::Single(monitor_match.clone());
    let result = notification_service.execute(payload).await;

    // The final result should be an error because the mock never returns a success
    // status.
    assert!(result.is_err());
    mock.assert();
}

#[tokio::test]
async fn test_failure_with_non_retryable_error() {
    let mut server = mockito::Server::new_async().await;

    let retry_policy = HttpRetryConfig { max_retry: 3, ..Default::default() };

    let mock_discord_notifier = NotifierBuilder::new("test_discord_no_retry")
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
    let notifiers =
        Arc::new(vec![mock_discord_notifier].into_iter().map(|n| (n.name.clone(), n)).collect());
    let notification_service = NotificationService::new(notifiers, http_client_pool);

    let monitor_match = MonitorMatch::new_tx_match(
        3,
        "Test Monitor".to_string(),
        "test_discord_no_retry".to_string(),
        789,
        Default::default(),
        json!({}),
    );

    let payload = NotificationPayload::Single(monitor_match.clone());
    let result = notification_service.execute(payload).await;

    assert!(result.is_err());
    mock.assert();
}

#[tokio::test]
async fn test_failure_with_invalid_url() {
    let mock_discord_notifier = NotifierBuilder::new("test_invalid_url")
        .discord_config("http://127.0.0.1:1") // Invalid URL that will fail to connect
        .retry_policy(HttpRetryConfig {
            max_retry: 0,
            initial_backoff_ms: std::time::Duration::from_millis(1),
            ..Default::default()
        })
        .build();

    let http_client_pool = Arc::new(HttpClientPool::default());
    let notifiers =
        Arc::new(vec![mock_discord_notifier].into_iter().map(|t| (t.name.clone(), t)).collect());
    let notification_service = NotificationService::new(notifiers, http_client_pool);

    let monitor_match = MonitorMatch::new_tx_match(
        4,
        "Test Monitor".to_string(),
        "test_invalid_url".to_string(),
        101,
        Default::default(),
        json!({}),
    );

    let payload = NotificationPayload::Single(monitor_match.clone());
    let result = notification_service.execute(payload).await;

    assert!(result.is_err());
}
