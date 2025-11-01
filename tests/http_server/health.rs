use crate::helpers::*;

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
