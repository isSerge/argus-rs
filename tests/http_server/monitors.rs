use crate::helpers::*;

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

    let resp = server.get("/monitors").await;
    assert_eq!(resp.status(), 500);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["error"], "An internal server error occurred");

    server.cleanup();
}

#[tokio::test]
async fn monitor_by_id_endpoint_handles_db_error() {
    let repo = create_test_repo_without_migrations().await;
    let server = TestServer::new(repo).await;

    let resp = server.get("/monitors/1").await;
    assert_eq!(resp.status(), 500);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["error"], "An internal server error occurred");

    server.cleanup();
}

#[tokio::test]
async fn create_monitor_endpoint_requires_auth() {
    let repo = create_test_repo().await;
    let server = TestServer::new(repo).await;

    let monitor_json = serde_json::json!({
        "name": "New Monitor",
        "network": "testnet",
        "address": serde_json::Value::Null,
        "abi_name": serde_json::Value::Null,
        "filter_script": "true",
        "actions": []
    });

    // No auth header
    let resp = server.post("/monitors").await.json(&monitor_json).send().await.unwrap();
    assert_eq!(resp.status(), 401);

    // Invalid token
    let resp = server
        .post("/monitors")
        .await
        .bearer_auth("invalid-key")
        .json(&monitor_json)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    server.cleanup();
}

#[tokio::test]
async fn create_monitor_endpoint_works() {
    let repo = create_test_repo().await;
    let server = TestServer::new(repo).await;

    let monitor_json = serde_json::json!({
        "name": "New Monitor",
        "network": "testnet",
        "address": serde_json::Value::Null,
        "abi_name": serde_json::Value::Null,
        "filter_script": "true",
        "actions": []
    });

    // 1. Successful creation
    let resp = server
        .post("/monitors")
        .await
        .bearer_auth("test-key")
        .json(&monitor_json)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "Failed to create monitor");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "Monitor creation triggered");

    // 2. Name conflict
    let resp = server
        .post("/monitors")
        .await
        .bearer_auth("test-key")
        .json(&monitor_json)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409, "Should return conflict for duplicate name");

    server.cleanup();
}

#[tokio::test]
async fn create_monitor_endpoint_validates_input() {
    let repo = create_test_repo().await;
    let server = TestServer::new(repo).await;

    // Missing body
    let resp = server.post("/monitors").await.bearer_auth("test-key").send().await.unwrap();
    assert!(resp.status().is_client_error(), "Expected 4xx error for missing body");

    // Invalid script syntax
    let invalid_monitor_json = serde_json::json!({
        "name": "Invalid Monitor",
        "network": "testnet",
        "address": serde_json::Value::Null,
        "abi_name": serde_json::Value::Null,
        "filter_script": "this is not valid rhai syntax !!@#",
        "actions": []
    });
    let resp = server
        .post("/monitors")
        .await
        .bearer_auth("test-key")
        .json(&invalid_monitor_json)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 422, "Should return unprocessable entity for invalid script");

    server.cleanup();
}

#[tokio::test]
async fn update_monitor_endpoint_requires_auth() {
    let (server, _repo) = TestServer::new_with_test_monitors().await;

    let updated_monitor_json = serde_json::json!({
        "name": "Updated Monitor",
        "network": "testnet",
        "address": serde_json::Value::Null,
        "abi_name": serde_json::Value::Null,
        "filter_script": "false",
        "actions": []
    });

    // No auth header
    let resp = server
        .client
        .put(&format!("http://{}/monitors/1", server.address))
        .json(&updated_monitor_json)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // Invalid token
    let resp = server
        .client
        .put(&format!("http://{}/monitors/1", server.address))
        .bearer_auth("invalid-key")
        .json(&updated_monitor_json)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    server.cleanup();
}

#[tokio::test]
async fn update_monitor_endpoint_works() {
    let (server, _repo) = TestServer::new_with_test_monitors().await;

    let updated_monitor_json = serde_json::json!({
        "name": "Updated Monitor",
        "network": "testnet",
        "address": serde_json::Value::Null,
        "abi_name": serde_json::Value::Null,
        "filter_script": "false",
        "actions": []
    });

    // Successful update
    let resp = server
        .client
        .put(&format!("http://{}/monitors/1", server.address))
        .bearer_auth("test-key")
        .json(&updated_monitor_json)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "Failed to update monitor");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "Monitors update triggered");

    server.cleanup();
}

#[tokio::test]
async fn update_monitor_endpoint_returns_404_for_nonexistent() {
    let repo = create_test_repo().await;
    let server = TestServer::new(repo).await;

    let updated_monitor_json = serde_json::json!({
        "name": "Updated Monitor",
        "network": "testnet",
        "address": serde_json::Value::Null,
        "abi_name": serde_json::Value::Null,
        "filter_script": "false",
        "actions": []
    });

    let resp = server
        .client
        .put(&format!("http://{}/monitors/999", server.address))
        .bearer_auth("test-key")
        .json(&updated_monitor_json)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404, "Should return not found for non-existent monitor");

    server.cleanup();
}

#[tokio::test]
async fn delete_monitor_endpoint_requires_auth() {
    let (server, _repo) = TestServer::new_with_test_monitors().await;

    // No auth header
    let resp = server
        .client
        .delete(&format!("http://{}/monitors/1", server.address))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // Invalid token
    let resp = server
        .client
        .delete(&format!("http://{}/monitors/1", server.address))
        .bearer_auth("invalid-key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    server.cleanup();
}

#[tokio::test]
async fn delete_monitor_endpoint_works() {
    let (server, _repo) = TestServer::new_with_test_monitors().await;

    // Successful deletion
    let resp = server
        .client
        .delete(&format!("http://{}/monitors/1", server.address))
        .bearer_auth("test-key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204, "Failed to delete monitor");

    // Verify deletion
    let resp = server.get("/monitors/1").await;
    assert_eq!(resp.status(), 404, "Monitor should be deleted");

    server.cleanup();
}

#[tokio::test]
async fn delete_monitor_endpoint_returns_404_for_nonexistent() {
    let repo = create_test_repo().await;
    let server = TestServer::new(repo).await;

    let resp = server
        .client
        .delete(&format!("http://{}/monitors/999", server.address))
        .bearer_auth("test-key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404, "Should return not found for non-existent monitor");

    server.cleanup();
}
