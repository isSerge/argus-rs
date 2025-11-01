use crate::helpers::*;

#[tokio::test]
async fn status_endpoint_returns_status_json() {
    let repo = create_test_repo().await;
    let server = TestServer::new(repo).await;

    let resp = server.get("/status").await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["network_id"], "testnet");
    assert!(body["uptime_secs"].as_u64().is_some());
    assert_eq!(body["latest_processed_block"], 0);
    assert_eq!(body["latest_processed_block_timestamp_secs"], 0);

    server.cleanup();
}
