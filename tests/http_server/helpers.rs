use std::{net::SocketAddr, sync::Arc};

use argus::{
    config::{AppConfig, ServerConfig},
    context::AppMetrics,
    http_server,
    models::{
        action::{ActionConfig, ActionTypeConfig, StdoutConfig},
        monitor::{MonitorConfig, MonitorStatus},
    },
    persistence::{sqlite::SqliteStateRepository, traits::AppRepository},
    test_helpers::create_monitor_validator,
};
use reqwest::Client;
use tokio::{sync::watch, task};

pub async fn create_test_repo() -> Arc<SqliteStateRepository> {
    let repo = SqliteStateRepository::new("sqlite::memory:")
        .await
        .expect("Failed to create in-memory repo");
    repo.run_migrations().await.expect("Failed to run migrations");
    Arc::new(repo)
}

pub async fn create_test_repo_without_migrations() -> Arc<SqliteStateRepository> {
    let repo = SqliteStateRepository::new("sqlite::memory:")
        .await
        .expect("Failed to create in-memory repo");
    Arc::new(repo)
}

pub fn create_test_server_config(address: &str) -> Arc<AppConfig> {
    Arc::new(AppConfig {
        network_id: "testnet".to_string(),
        server: ServerConfig { listen_address: address.into(), ..Default::default() },
        ..Default::default()
    })
}

pub struct TestServer {
    pub address: SocketAddr,
    pub server_handle: task::JoinHandle<()>,
    pub client: Client,
    _config_rx: watch::Receiver<()>,
}

impl TestServer {
    pub async fn new(repo: Arc<dyn AppRepository>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind");
        let addr = listener.local_addr().expect("Failed to get address");
        drop(listener); // Release port for the app to use

        let mut config = create_test_server_config(&addr.to_string()).as_ref().clone();
        config.server.api_key = Some("test-key".to_string());
        let config = Arc::new(config);

        let metrics = AppMetrics::default();
        let (config_tx, config_rx) = watch::channel(());

        // Create a basic MonitorValidator for tests
        let actions = repo.get_actions(&config.network_id).await.unwrap_or_default();
        let monitor_validator = Arc::new(create_monitor_validator(&actions, None).await);

        // Spawn the actual app server
        let server_handle = task::spawn(async move {
            http_server::run_server_from_config(
                config,
                repo,
                metrics,
                config_tx,
                monitor_validator,
            )
            .await;
        });

        // Wait for server to start
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        Self { address: addr, server_handle, client: Client::new(), _config_rx: config_rx }
    }

    pub async fn new_with_test_monitors() -> (Self, Arc<SqliteStateRepository>) {
        let repo = create_test_repo().await;
        let network_id = "testnet".to_string();

        // Add test monitors
        let add_result = repo
            .add_monitors(
                &network_id,
                vec![MonitorConfig {
                    name: "Test Monitor".to_string(),
                    network: network_id.clone(),
                    address: None,
                    abi_name: None,
                    filter_script: "true".to_string(),
                    actions: vec![],
                    status: MonitorStatus::default(),
                }],
            )
            .await;
        assert!(add_result.is_ok(), "Failed to add test monitor");

        let server = Self::new(repo.clone()).await;
        (server, repo)
    }

    pub async fn new_with_multiple_monitors() -> (Self, Arc<SqliteStateRepository>) {
        let repo = create_test_repo().await;
        let network_id = "testnet".to_string();

        // Add test monitors
        let add_result = repo
            .add_monitors(
                &network_id,
                vec![
                    MonitorConfig {
                        name: "Test Monitor".to_string(),
                        network: network_id.clone(),
                        address: None,
                        abi_name: None,
                        filter_script: "true".to_string(),
                        actions: vec![],
                        status: MonitorStatus::default(),
                    },
                    MonitorConfig {
                        name: "Another Monitor".to_string(),
                        network: network_id.clone(),
                        address: None,
                        abi_name: None,
                        filter_script: "false".to_string(),
                        actions: vec![],
                        status: MonitorStatus::default(),
                    },
                ],
            )
            .await;
        assert!(add_result.is_ok(), "Failed to add test monitors");

        let server = Self::new(repo.clone()).await;
        (server, repo)
    }

    pub async fn new_with_test_actions() -> (Self, Arc<SqliteStateRepository>) {
        let repo = create_test_repo().await;
        let network_id = "testnet".to_string();

        // Add test actions
        let add_result = repo
            .create_action(
                &network_id,
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

    pub async fn new_with_multiple_actions() -> (Self, Arc<SqliteStateRepository>) {
        let repo = create_test_repo().await;
        let network_id = "testnet".to_string();

        // Add test actions
        let add_result_1 = repo
            .create_action(
                &network_id,
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
                &network_id,
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

    pub async fn new_with_test_abis() -> (Self, Arc<SqliteStateRepository>) {
        let repo = create_test_repo().await;

        // Add test ABI
        let abi_content = r#"[{"type":"function","name":"transfer","inputs":[{"name":"to","type":"address"},{"name":"amount","type":"uint256"}]}]"#;
        let add_result = repo.create_abi("Test ABI", abi_content).await;
        assert!(add_result.is_ok(), "Failed to add test ABI");

        let server = Self::new(repo.clone()).await;
        (server, repo)
    }

    pub async fn new_with_multiple_abis() -> (Self, Arc<SqliteStateRepository>) {
        let repo = create_test_repo().await;

        // Add test ABIs
        let abi1_content = r#"[{"type":"function","name":"transfer"}]"#;
        let abi2_content = r#"[{"type":"function","name":"approve"}]"#;
        let add_result_1 = repo.create_abi("Test ABI", abi1_content).await;
        let add_result_2 = repo.create_abi("Another ABI", abi2_content).await;

        assert!(add_result_1.is_ok(), "Failed to add test ABIs");
        assert!(add_result_2.is_ok(), "Failed to add test ABIs");

        let server = Self::new(repo.clone()).await;
        (server, repo)
    }

    pub async fn get(&self, path: &str) -> reqwest::Response {
        let url = format!("http://{}{}", self.address, path);
        self.client.get(&url).send().await.expect("Request failed")
    }

    pub async fn post(&self, path: &str) -> reqwest::RequestBuilder {
        let url = format!("http://{}{}", self.address, path);
        self.client.post(&url)
    }

    pub fn cleanup(self) {
        self.server_handle.abort();
    }
}
