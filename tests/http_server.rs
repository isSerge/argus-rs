use std::sync::Arc;

use argus::{
    config::{AppConfig, ServerConfig},
    http_server,
    models::{
        action::{ActionConfig, ActionTypeConfig, StdoutConfig},
        monitor::MonitorConfig,
    },
    persistence::{sqlite::SqliteStateRepository, traits::AppRepository},
};
use reqwest::Client;
use tokio::task;

async fn create_test_repo() -> Arc<SqliteStateRepository> {
    let repo = SqliteStateRepository::new("sqlite::memory:")
        .await
        .expect("Failed to create in-memory repo");
    repo.run_migrations().await.expect("Failed to run migrations");
    Arc::new(repo)
}

fn create_test_server_config(address: &str) -> Arc<AppConfig> {
    Arc::new(AppConfig {
        server: ServerConfig { enabled: true, listen_address: address.into() },
        ..Default::default()
    })
}

#[tokio::test]
async fn health_endpoint_returns_ok() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind");
    let addr = listener.local_addr().expect("Failed to get address");
    drop(listener); // Release port for the app to use

    let config = create_test_server_config(&addr.to_string());
    let repo = create_test_repo().await;

    // Spawn the actual app server
    let server_handle = task::spawn(async move {
        http_server::run_server_from_config(config, repo).await;
    });

    // Wait for server to start
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Test the endpoint
    let url = format!("http://{}/health", addr);
    let client = Client::new();
    let resp = client.get(&url).send().await.expect("Request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["status"], "ok");

    // Clean up
    server_handle.abort();
}

#[tokio::test]
async fn monitors_endpoint_returns_empty_list() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind");
    let addr = listener.local_addr().expect("Failed to get address");
    drop(listener); // Release port for the app to use

    let config = create_test_server_config(&addr.to_string());
    let repo = create_test_repo().await;

    // Spawn the actual app server
    let server_handle = task::spawn(async move {
        http_server::run_server_from_config(config, repo).await;
    });

    // Wait for server to start
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Test the endpoint
    let url = format!("http://{}/monitors", addr);
    let client = Client::new();
    let resp = client.get(&url).send().await.expect("Request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["monitors"], serde_json::Value::Array(vec![]));

    // Clean up
    server_handle.abort();
}

#[tokio::test]
async fn monitor_by_id_endpoint_returns_404_for_nonexistent_id() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind");
    let addr = listener.local_addr().expect("Failed to get address");
    drop(listener); // Release port for the app to use

    let config = create_test_server_config(&addr.to_string());
    let repo = create_test_repo().await;

    // Spawn the actual app server
    let server_handle = task::spawn(async move {
        http_server::run_server_from_config(config, repo).await;
    });

    // Wait for server to start
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Test the endpoint
    let url = format!("http://{}/monitors/1234", addr);
    let client = Client::new();
    let resp = client.get(&url).send().await.expect("Request failed");

    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["error"], "Monitor not found");

    // Clean up
    server_handle.abort();
}

#[tokio::test]
async fn monitor_by_id_endpoint_returns_monitor_when_exists() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind");
    let addr = listener.local_addr().expect("Failed to get address");
    drop(listener); // Release port for the app to use

    let config = create_test_server_config(&addr.to_string());
    let repo = create_test_repo().await;

    // Add a test monitor to the repo
    let add_result = repo
        .add_monitors(
            &config.network_id,
            vec![MonitorConfig {
                name: "Test Monitor".to_string(),
                network: config.network_id.clone(),
                address: None,
                abi: None,
                filter_script: "true".to_string(),
                actions: vec![],
            }],
        )
        .await;

    assert!(add_result.is_ok(), "Failed to add test monitor");

    // Spawn the actual app server
    let server_handle = task::spawn(async move {
        http_server::run_server_from_config(config, repo).await;
    });

    // Wait for server to start
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Test the endpoint
    let url = format!("http://{}/monitors/1", addr);
    let client = Client::new();
    let resp = client.get(&url).send().await.expect("Request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["monitor"]["id"], 1);
    assert_eq!(body["monitor"]["name"], "Test Monitor");

    // Clean up
    server_handle.abort();
}

#[tokio::test]
async fn monitors_returns_list_of_monitors_when_exist() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind");
    let addr = listener.local_addr().expect("Failed to get address");
    drop(listener); // Release port for the app to use

    let config = create_test_server_config(&addr.to_string());
    let repo = create_test_repo().await;

    // Add a test monitor to the repo
    let add_result = repo
        .add_monitors(
            &config.network_id,
            vec![
                MonitorConfig {
                    name: "Test Monitor".to_string(),
                    network: config.network_id.clone(),
                    address: None,
                    abi: None,
                    filter_script: "true".to_string(),
                    actions: vec![],
                },
                MonitorConfig {
                    name: "Another Monitor".to_string(),
                    network: config.network_id.clone(),
                    address: None,
                    abi: None,
                    filter_script: "false".to_string(),
                    actions: vec![],
                },
            ],
        )
        .await;

    assert!(add_result.is_ok(), "Failed to add test monitor");

    // Spawn the actual app server
    let server_handle = task::spawn(async move {
        http_server::run_server_from_config(config, repo).await;
    });

    // Wait for server to start
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Test the endpoint
    let url = format!("http://{}/monitors", addr);
    let client = Client::new();
    let resp = client.get(&url).send().await.expect("Request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["monitors"].as_array().unwrap().len(), 2);
    assert_eq!(body["monitors"][0]["id"], 1);
    assert_eq!(body["monitors"][0]["name"], "Test Monitor");
    assert_eq!(body["monitors"][1]["id"], 2);
    assert_eq!(body["monitors"][1]["name"], "Another Monitor");

    // Clean up
    server_handle.abort();
}

#[tokio::test]
async fn monitors_endpoint_handles_db_error() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind");
    let addr = listener.local_addr().expect("Failed to get address");
    drop(listener); // Release port for the app to use

    let config = create_test_server_config(&addr.to_string());

    // Create a repo but do not run migrations to simulate a DB error
    let repo = Arc::new(
        SqliteStateRepository::new("sqlite::memory:")
            .await
            .expect("Failed to create in-memory repo"),
    );

    // Spawn the actual app server
    let server_handle = task::spawn(async move {
        http_server::run_server_from_config(config, repo).await;
    });

    // Wait for server to start
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Test the /monitors endpoint
    let url = format!("http://{}/monitors", addr);
    let client = Client::new();
    let resp = client.get(&url).send().await.expect("Request failed");
    assert_eq!(resp.status(), 500);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["error"], "An internal server error occurred");

    // Test the /monitors/{id} endpoint
    let url = format!("http://{}/monitors/1", addr);
    let resp = client.get(&url).send().await.expect("Request failed");
    assert_eq!(resp.status(), 500);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["error"], "An internal server error occurred");

    // Clean up
    server_handle.abort();
}

#[tokio::test]
async fn actions_endpoint_returns_empty_list() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind");
    let addr = listener.local_addr().expect("Failed to get address");
    drop(listener);

    let config = create_test_server_config(&addr.to_string());
    let repo = create_test_repo().await;

    let server_handle = task::spawn(async move {
        http_server::run_server_from_config(config, repo).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let url = format!("http://{}/actions", addr);
    let client = Client::new();
    let resp = client.get(&url).send().await.expect("Request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["actions"], serde_json::Value::Array(vec![]));

    server_handle.abort();
}

#[tokio::test]
async fn action_by_id_endpoint_returns_404_for_nonexistent_id() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind");
    let addr = listener.local_addr().expect("Failed to get address");
    drop(listener);

    let config = create_test_server_config(&addr.to_string());
    let repo = create_test_repo().await;

    let server_handle = task::spawn(async move {
        http_server::run_server_from_config(config, repo).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let url = format!("http://{}/actions/1234", addr);
    let client = Client::new();
    let resp = client.get(&url).send().await.expect("Request failed");

    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["error"], "Action not found");

    server_handle.abort();
}

#[tokio::test]
async fn action_by_id_endpoint_returns_action_when_exists() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind");
    let addr = listener.local_addr().expect("Failed to get address");
    drop(listener);

    let config = create_test_server_config(&addr.to_string());
    let repo = create_test_repo().await;

    let add_result = repo
        .add_actions(
            &config.network_id,
            vec![ActionConfig {
                id: None,
                name: "Test Action".to_string(),
                config: ActionTypeConfig::Stdout(StdoutConfig { message: None }),
                policy: None,
            }],
        )
        .await;
    assert!(add_result.is_ok(), "Failed to add test action");

    let server_handle = task::spawn(async move {
        http_server::run_server_from_config(config, repo).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let url = format!("http://{}/actions/1", addr);
    let client = Client::new();
    let resp = client.get(&url).send().await.expect("Request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["action"]["id"], 1);
    assert_eq!(body["action"]["name"], "Test Action");

    server_handle.abort();
}

#[tokio::test]
async fn actions_returns_list_of_actions_when_exist() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind");
    let addr = listener.local_addr().expect("Failed to get address");
    drop(listener);

    let config = create_test_server_config(&addr.to_string());
    let repo = create_test_repo().await;

    let add_result = repo
        .add_actions(
            &config.network_id,
            vec![
                ActionConfig {
                    id: None,
                    name: "Test Action".to_string(),
                    config: ActionTypeConfig::Stdout(StdoutConfig { message: None }),
                    policy: None,
                },
                ActionConfig {
                    id: None,
                    name: "Another Action".to_string(),
                    config: ActionTypeConfig::Stdout(StdoutConfig { message: None }),
                    policy: None,
                },
            ],
        )
        .await;
    assert!(add_result.is_ok(), "Failed to add test actions");

    let server_handle = task::spawn(async move {
        http_server::run_server_from_config(config, repo).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let url = format!("http://{}/actions", addr);
    let client = Client::new();
    let resp = client.get(&url).send().await.expect("Request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["actions"].as_array().unwrap().len(), 2);
    assert_eq!(body["actions"][0]["id"], 1);
    assert_eq!(body["actions"][0]["name"], "Test Action");
    assert_eq!(body["actions"][1]["id"], 2);
    assert_eq!(body["actions"][1]["name"], "Another Action");

    server_handle.abort();
}

#[tokio::test]
async fn actions_endpoint_handles_db_error() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind");
    let addr = listener.local_addr().expect("Failed to get address");
    drop(listener);

    let config = create_test_server_config(&addr.to_string());

    let repo = Arc::new(
        SqliteStateRepository::new("sqlite::memory:")
            .await
            .expect("Failed to create in-memory repo"),
    );

    let server_handle = task::spawn(async move {
        http_server::run_server_from_config(config, repo).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let url = format!("http://{}/actions", addr);
    let client = Client::new();
    let resp = client.get(&url).send().await.expect("Request failed");
    assert_eq!(resp.status(), 500);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["error"], "An internal server error occurred");

    let url = format!("http://{}/actions/1", addr);
    let resp = client.get(&url).send().await.expect("Request failed");
    assert_eq!(resp.status(), 500);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["error"], "An internal server error occurred");

    server_handle.abort();
}
