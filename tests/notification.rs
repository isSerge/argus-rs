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
    notification::NotificationService,
};
use mockito;
use serde_json::json;

#[tokio::test]
async fn test_success() {
    let mut server = mockito::Server::new_async().await;

    let mock_discord_notifier = NotifierConfig {
        name: "test_discord".to_string(),
        config: NotifierTypeConfig::Discord(DiscordConfig {
            discord_url: server.url(),
            message: NotificationMessage {
                title: "Test Title".to_string(),
                body: "Monitor {{ monitor_id }} matched on block {{ block_number }}".to_string(),
            },
            retry_policy: Default::default(),
        }),
    };

    let mock = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"content":"*Test Title*\n\nMonitor 1 matched on block 123"}"#)
        .create_async()
        .await;

    let http_client_pool = Arc::new(HttpClientPool::new());
    let notification_service =
        NotificationService::new(vec![mock_discord_notifier], http_client_pool);

    let monitor_match = MonitorMatch {
        monitor_id: 1,
        block_number: 123,
        transaction_hash: Default::default(),
        contract_address: Default::default(),
        notifier_name: "test_discord".to_string(),
        trigger_data: json!({}),
        log_index: None,
    };

    let result = notification_service.execute(&monitor_match).await;

    assert!(result.is_ok());
    mock.assert();
}

#[tokio::test]
async fn test_failure_with_retryable_error() {
    let mut server = mockito::Server::new_async().await;

    let retry_policy = HttpRetryConfig { max_retries: 2, ..Default::default() };

    let mock_discord_notifier = NotifierConfig {
        name: "test_discord_retry".to_string(),
        config: NotifierTypeConfig::Discord(DiscordConfig {
            discord_url: server.url(),
            message: NotificationMessage {
                title: "Retry Test".to_string(),
                body: "This should retry".to_string(),
            },
            retry_policy: retry_policy.clone(),
        }),
    };

    // Mock a server that fails twice with a retryable error, then succeeds.
    let mock = server
        .mock("POST", "/")
        .with_status(503)
        .with_body("Service Unavailable")
        .expect(3)
        .create_async()
        .await;

    let http_client_pool = Arc::new(HttpClientPool::new());
    let notification_service =
        NotificationService::new(vec![mock_discord_notifier], http_client_pool);

    let monitor_match = MonitorMatch {
        monitor_id: 2,
        block_number: 456,
        notifier_name: "test_discord_retry".to_string(),
        transaction_hash: Default::default(),
        contract_address: Default::default(),
        trigger_data: json!({}),
        log_index: None,
    };

    let result = notification_service.execute(&monitor_match).await;

    // The final result should be an error because the mock never returns a success
    // status.
    assert!(result.is_err());
    mock.assert();
}

#[tokio::test]
async fn test_failure_with_non_retryable_error() {
    let mut server = mockito::Server::new_async().await;

    let retry_policy = HttpRetryConfig { max_retries: 3, ..Default::default() };

    let mock_discord_notifier = NotifierConfig {
        name: "test_discord_no_retry".to_string(),
        config: NotifierTypeConfig::Discord(DiscordConfig {
            discord_url: server.url(),
            message: NotificationMessage {
                title: "No Retry Test".to_string(),
                body: "This should not retry".to_string(),
            },
            retry_policy: retry_policy.clone(),
        }),
    };

    // Mock a server that returns a 400 Bad Request, which is a non-retryable error.
    let mock = server
        .mock("POST", "/")
        .with_status(400)
        .with_body("Bad Request")
        .expect(1) // Should only be called once
        .create_async()
        .await;

    let http_client_pool = Arc::new(HttpClientPool::new());
    let notification_service =
        NotificationService::new(vec![mock_discord_notifier], http_client_pool);

    let monitor_match = MonitorMatch {
        monitor_id: 3,
        block_number: 789,
        notifier_name: "test_discord_no_retry".to_string(),
        transaction_hash: Default::default(),
        contract_address: Default::default(),
        trigger_data: json!({}),
        log_index: None,
    };

    let result = notification_service.execute(&monitor_match).await;

    assert!(result.is_err());
    mock.assert();
}

#[tokio::test]
async fn test_failure_with_invalid_url() {
    let mock_discord_notifier = NotifierConfig {
        name: "test_invalid_url".to_string(),
        config: NotifierTypeConfig::Discord(DiscordConfig {
            discord_url: "http://127.0.0.1:1".to_string(), // URL with invalid port
            message: NotificationMessage {
                title: "Invalid URL Test".to_string(),
                body: "This should fail".to_string(),
            },
            retry_policy: HttpRetryConfig {
                max_retries: 0,
                initial_backoff_ms: std::time::Duration::from_millis(1),
                ..Default::default()
            },
        }),
    };

    let http_client_pool = Arc::new(HttpClientPool::new());
    let notification_service =
        NotificationService::new(vec![mock_discord_notifier], http_client_pool);

    let monitor_match = MonitorMatch {
        monitor_id: 4,
        block_number: 101,
        notifier_name: "test_invalid_url".to_string(),
        transaction_hash: Default::default(),
        contract_address: Default::default(),
        trigger_data: json!({}),
        log_index: None,
    };

    let result = notification_service.execute(&monitor_match).await;

    assert!(result.is_err());
}
