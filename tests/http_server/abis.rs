use argus::{
    models::monitor::{MonitorConfig, MonitorStatus},
    persistence::traits::AppRepository,
};

use crate::helpers::*;

#[tokio::test]
async fn abis_endpoint_returns_empty_list() {
    let repo = create_test_repo().await;
    let server = TestServer::new(repo).await;

    let resp = server.get("/abis").await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["abis"], serde_json::Value::Array(vec![]));

    server.cleanup();
}

#[tokio::test]
async fn abi_by_name_endpoint_returns_404_for_nonexistent_name() {
    let repo = create_test_repo().await;
    let server = TestServer::new(repo).await;

    let resp = server.get("/abis/nonexistent").await;

    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["error"], "ABI not found");

    server.cleanup();
}

#[tokio::test]
async fn abi_by_name_endpoint_returns_abi_when_exists() {
    let (server, _repo) = TestServer::new_with_test_abis().await;

    let resp = server.get("/abis/Test%20ABI").await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["abi"]["name"], "Test ABI");
    assert!(body["abi"]["abi"].is_string());

    server.cleanup();
}

#[tokio::test]
async fn abis_returns_list_of_abis_when_exist() {
    let (server, _repo) = TestServer::new_with_multiple_abis().await;

    let resp = server.get("/abis").await;

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    let abis = body["abis"].as_array().unwrap();
    assert_eq!(abis.len(), 2);
    assert_eq!(abis[0], "Another ABI");
    assert_eq!(abis[1], "Test ABI");

    server.cleanup();
}

#[tokio::test]
async fn upload_abi_endpoint_works() {
    let repo = create_test_repo().await;
    let server = TestServer::new(repo).await;
    let abi_json = serde_json::json!({
        "name": "TestABI",
        "abi": r#"[{"type":"function","name":"transfer","inputs":[{"name":"to","type":"address"},{"name":"amount","type":"uint256"}]}]"#
    });

    // 1. Successful creation
    let resp =
        server.post("/abis").await.bearer_auth("test-key").json(&abi_json).send().await.unwrap();
    assert_eq!(resp.status(), 201, "Failed to create ABI");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["abi"]["name"], "TestABI");
    assert!(body["abi"]["abi"].is_string());

    // 2. Name conflict
    let resp =
        server.post("/abis").await.bearer_auth("test-key").json(&abi_json).send().await.unwrap();
    assert_eq!(resp.status(), 409, "Should return conflict for duplicate name");

    // 3. Invalid JSON
    let invalid_abi_json = serde_json::json!({
        "name": "InvalidABI",
        "abi": "not valid json"
    });
    let resp = server
        .post("/abis")
        .await
        .bearer_auth("test-key")
        .json(&invalid_abi_json)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 422, "Should return unprocessable entity for invalid JSON");

    server.cleanup();
}

#[tokio::test]
async fn delete_abi_endpoint_works() {
    let (server, _repo) = TestServer::new_with_test_abis().await;

    // 1. Successful deletion
    let resp = server
        .client
        .delete(&format!("http://{}{}", server.address, "/abis/Test%20ABI"))
        .bearer_auth("test-key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204, "Failed to delete ABI");

    // 2. Verify deletion
    let resp = server.get("/abis/Test%20ABI").await;
    assert_eq!(resp.status(), 404, "ABI should be deleted");

    // 3. Delete non-existent ABI
    let resp = server
        .client
        .delete(&format!("http://{}{}", server.address, "/abis/NonExistent"))
        .bearer_auth("test-key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404, "Should return not found for non-existent ABI");

    server.cleanup();
}

#[tokio::test]
async fn abis_write_endpoints_require_auth() {
    let (server, _repo) = TestServer::new_with_test_abis().await;
    let json_body = serde_json::json!({
        "name": "TestABI",
        "abi": "[]"
    });

    // 1. POST /abis
    let resp = server.post("/abis").await.json(&json_body).send().await.unwrap();
    assert_eq!(resp.status(), 401, "POST /abis should require auth");

    // 2. DELETE /abis/:name
    let resp = server
        .client
        .delete(&format!("http://{}{}", server.address, "/abis/TestABI"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "DELETE /abis/:name should require auth");

    server.cleanup();
}

#[tokio::test]
async fn abis_endpoint_handles_db_error() {
    let repo = create_test_repo_without_migrations().await;
    let server = TestServer::new(repo).await;

    let resp = server.get("/abis").await;
    assert_eq!(resp.status(), 500);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["error"], "An internal server error occurred");

    let resp = server.get("/abis/TestABI").await;
    assert_eq!(resp.status(), 500);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["error"], "An internal server error occurred");

    server.cleanup();
}

#[tokio::test]
async fn delete_abi_conflict_when_in_use() {
    let repo = create_test_repo().await;
    let network_id = "testnet".to_string();

    // Create a test ABI
    let abi_content = r#"[{"type":"event","name":"Transfer","inputs":[]}]"#;
    repo.create_abi("ERC20", abi_content).await.expect("Failed to create ABI");

    // Create a monitor that uses the ABI
    let monitor = MonitorConfig {
        name: "ERC20 Monitor".to_string(),
        network: network_id.clone(),
        address: Some("0x1234567890123456789012345678901234567890".to_string()),
        abi_name: Some("ERC20".to_string()),
        filter_script: "true".to_string(),
        actions: vec![],
        status: MonitorStatus::default(),
    };
    repo.add_monitors(&network_id, vec![monitor]).await.unwrap();

    let server = TestServer::new(repo).await;

    // 1. Attempt to delete the ABI that is in use
    let resp = server
        .client
        .delete(&format!("http://{}{}", server.address, "/abis/ERC20"))
        .bearer_auth("test-key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409, "Should return conflict when ABI is in use");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "ABI is in use and cannot be deleted.");
    assert_eq!(body["monitors"], serde_json::json!(vec!["ERC20 Monitor"]));

    server.cleanup();
}
