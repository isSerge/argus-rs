use tokio::task;
use reqwest::Client;
use argus::config::AppConfig;
use argus::http_server;

#[tokio::test]
async fn health_endpoint_returns_ok() {
    // Find an available port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind");
    let addr = listener.local_addr().expect("Failed to get address");
    drop(listener); // Release port for the app to use

    let config = AppConfig {
        api_server_listen_address: addr.to_string(),
        ..Default::default()
    };

    // Spawn the actual app server
    let server_handle = task::spawn(async move {
        http_server::run_server_from_config(&config).await;
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
