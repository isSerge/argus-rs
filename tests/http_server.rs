use std::{net::SocketAddr, sync::Arc};

use argus::{
    config::{AppConfig, ServerConfig},
    context::AppMetrics,
    http_server,
    models::{
        action::{ActionConfig, ActionTypeConfig, StdoutConfig},
        monitor::MonitorConfig,
    },
    persistence::{sqlite::SqliteStateRepository, traits::AppRepository},
};
use reqwest::Client;
use tokio::{sync::watch, task};

async fn create_test_repo() -> Arc<SqliteStateRepository> {
    let repo = SqliteStateRepository::new("sqlite::memory:")
        .await
        .expect("Failed to create in-memory repo");
    repo.run_migrations().await.expect("Failed to run migrations");
    Arc::new(repo)
}

async fn create_test_repo_without_migrations() -> Arc<SqliteStateRepository> {
    let repo = SqliteStateRepository::new("sqlite::memory:")
        .await
        .expect("Failed to create in-memory repo");
    Arc::new(repo)
}

fn create_test_server_config(address: &str) -> Arc<AppConfig> {
    Arc::new(AppConfig {
        server: ServerConfig { listen_address: address.into(), ..Default::default() },
        ..Default::default()
    })
}

struct TestServer {
    pub address: SocketAddr,
    pub server_handle: task::JoinHandle<()>,
    pub client: Client,
    _config_rx: watch::Receiver<()>,
}

impl TestServer {
    async fn new(repo: Arc<dyn AppRepository>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind");
        let addr = listener.local_addr().expect("Failed to get address");
        drop(listener); // Release port for the app to use

        let mut config = create_test_server_config(&addr.to_string()).as_ref().clone();
        config.server.api_key = Some("test-key".to_string());
        let config = Arc::new(config);

        let metrics = AppMetrics::default();
        let (config_tx, config_rx) = watch::channel(());

        // Spawn the actual app server
        let server_handle = task::spawn(async move {
            http_server::run_server_from_config(config, repo, metrics, config_tx).await;
        });

        // Wait for server to start
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        Self { address: addr, server_handle, client: Client::new(), _config_rx: config_rx }
    }

    async fn new_with_test_monitors() -> (Self, Arc<SqliteStateRepository>) {
        let repo = create_test_repo().await;
        let config = AppConfig::default();

        // Add test monitors
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

        let server = Self::new(repo.clone()).await;
        (server, repo)
    }

    async fn new_with_multiple_monitors() -> (Self, Arc<SqliteStateRepository>) {
        let repo = create_test_repo().await;
        let config = AppConfig::default();

        // Add test monitors
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
        assert!(add_result.is_ok(), "Failed to add test monitors");

        let server = Self::new(repo.clone()).await;
        (server, repo)
    }

    async fn new_with_test_actions() -> (Self, Arc<SqliteStateRepository>) {
        let repo = create_test_repo().await;
        let config = AppConfig::default();

        // Add test actions
        let add_result = repo
            .create_action(
                &config.network_id,
                ActionConfig {
                    id: None,
                    name: "Test Action".to_string(),
                    config: ActionTypeConfig::Stdout(StdoutConfig { message: None }),
                    policy: None,
                },
            )
            .await;
        assert!(add_result.is_ok(), "Failed to add test action");

        let server = Self::new(repo.clone()).await;
        (server, repo)
    }

    async fn new_with_multiple_actions() -> (Self, Arc<SqliteStateRepository>) {
        let repo = create_test_repo().await;
        let config = AppConfig::default();

        // Add test actions
        let add_result_1 = repo
            .create_action(
                &config.network_id,
                    ActionConfig {
                        id: None,
                        name: "Test Action".to_string(),
                        config: ActionTypeConfig::Stdout(StdoutConfig { message: None }),
                        policy: None,
                    },
            )
            .await;

         let add_result_2 = repo
            .create_action(
                &config.network_id,
                    ActionConfig {
                        id: None,
                        name: "Another Action".to_string(),
                        config: ActionTypeConfig::Stdout(StdoutConfig { message: None }),
                        policy: None,
                    },
            )
            .await;

        assert!(add_result_1.is_ok(), "Failed to add test actions");
        assert!(add_result_2.is_ok(), "Failed to add test actions");

        let server = Self::new(repo.clone()).await;
        (server, repo)
    }

    async fn get(&self, path: &str) -> reqwest::Response {
        let url = format!("http://{}{}", self.address, path);
        self.client.get(&url).send().await.expect("Request failed")
    }

    async fn post(&self, path: &str) -> reqwest::RequestBuilder {
        let url = format!("http://{}{}", self.address, path);
        self.client.post(&url)
    }

    fn cleanup(self) {
        self.server_handle.abort();
    }
}

#[tokio::test]
async fn health_endpoint_returns_ok() {
    let repo = create_test_repo().await;
    let server = TestServer::new(repo).await;

    let resp = server.get("/health").await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["status"], "ok");

    server.cleanup();
}

#[tokio::test]
async fn monitors_endpoint_returns_empty_list() {
    let repo = create_test_repo().await;
    let server = TestServer::new(repo).await;

    let resp = server.get("/monitors").await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["monitors"], serde_json::Value::Array(vec![]));

    server.cleanup();
}

#[tokio::test]
async fn monitor_by_id_endpoint_returns_404_for_nonexistent_id() {
    let repo = create_test_repo().await;
    let server = TestServer::new(repo).await;

    let resp = server.get("/monitors/1234").await;

    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["error"], "Monitor not found");

    server.cleanup();
}

#[tokio::test]
async fn monitor_by_id_endpoint_returns_monitor_when_exists() {
    let (server, _repo) = TestServer::new_with_test_monitors().await;

    let resp = server.get("/monitors/1").await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["monitor"]["id"], 1);
    assert_eq!(body["monitor"]["name"], "Test Monitor");

    server.cleanup();
}

#[tokio::test]
async fn monitors_returns_list_of_monitors_when_exist() {
    let (server, _repo) = TestServer::new_with_multiple_monitors().await;

    let resp = server.get("/monitors").await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["monitors"].as_array().unwrap().len(), 2);
    assert_eq!(body["monitors"][0]["id"], 1);
    assert_eq!(body["monitors"][0]["name"], "Test Monitor");
    assert_eq!(body["monitors"][1]["id"], 2);
    assert_eq!(body["monitors"][1]["name"], "Another Monitor");

    server.cleanup();
}

#[tokio::test]
async fn monitors_endpoint_handles_db_error() {
    let repo = create_test_repo_without_migrations().await;
    let server = TestServer::new(repo).await;

    // Test the /monitors endpoint
    let resp = server.get("/monitors").await;
    assert_eq!(resp.status(), 500);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["error"], "An internal server error occurred");

    // Test the /monitors/{id} endpoint
    let resp = server.get("/monitors/1").await;
    assert_eq!(resp.status(), 500);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["error"], "An internal server error occurred");

    server.cleanup();
}

#[tokio::test]
async fn update_monitors_endpoint_requires_auth() {
    let repo = create_test_repo().await;
    let server = TestServer::new(repo).await;

    // 1. No auth header
    let resp = server.post("/monitors").await.send().await.unwrap();
    assert_eq!(resp.status(), 401);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["error"], "Unauthorized");

    // 2. Invalid token
    let resp = server.post("/monitors").await.bearer_auth("invalid-key").send().await.unwrap();
    assert_eq!(resp.status(), 401);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["error"], "Unauthorized");

    // 3. Valid token
    let resp = server.post("/monitors").await.bearer_auth("test-key").send().await.unwrap();
    assert_eq!(resp.status(), 200);

    server.cleanup();
}

#[tokio::test]
async fn actions_endpoint_returns_empty_list() {
    let repo = create_test_repo().await;
    let server = TestServer::new(repo).await;

    let resp = server.get("/actions").await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["actions"], serde_json::Value::Array(vec![]));

    server.cleanup();
}

#[tokio::test]
async fn action_by_id_endpoint_returns_404_for_nonexistent_id() {
    let repo = create_test_repo().await;
    let server = TestServer::new(repo).await;

    let resp = server.get("/actions/1234").await;

    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["error"], "Action not found");

    server.cleanup();
}

#[tokio::test]
async fn action_by_id_endpoint_returns_action_when_exists() {
    let (server, _repo) = TestServer::new_with_test_actions().await;

    let resp = server.get("/actions/1").await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["action"]["id"], 1);
    assert_eq!(body["action"]["name"], "Test Action");

    server.cleanup();
}

#[tokio::test]
async fn actions_returns_list_of_actions_when_exist() {
    let (server, _repo) = TestServer::new_with_multiple_actions().await;

    let resp = server.get("/actions").await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["actions"].as_array().unwrap().len(), 2);
    assert_eq!(body["actions"][0]["id"], 1);
    assert_eq!(body["actions"][0]["name"], "Test Action");
    assert_eq!(body["actions"][1]["id"], 2);
    assert_eq!(body["actions"][1]["name"], "Another Action");

    server.cleanup();
}

#[tokio::test]
async fn actions_endpoint_handles_db_error() {
    let repo = create_test_repo_without_migrations().await;
    let server = TestServer::new(repo).await;

    let resp = server.get("/actions").await;
    assert_eq!(resp.status(), 500);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["error"], "An internal server error occurred");

    let resp = server.get("/actions/1").await;
    assert_eq!(resp.status(), 500);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["error"], "An internal server error occurred");

    server.cleanup();
}

#[tokio::test]
async fn status_endpoint_returns_status_json() {
    let repo = create_test_repo().await;
    let server = TestServer::new(repo).await;

    let resp = server.get("/status").await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["network_id"], ""); // Default network_id is empty string
    assert!(body["uptime_secs"].as_u64().is_some());
    assert_eq!(body["latest_processed_block"], 0);
    assert_eq!(body["latest_processed_block_timestamp_secs"], 0);

    server.cleanup();
}
