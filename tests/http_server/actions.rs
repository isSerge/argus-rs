use crate::helpers::*;

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
async fn create_action_endpoint_works() {
    let repo = create_test_repo().await;
    let server = TestServer::new(repo).await;
    let action_json = serde_json::json!({
        "name": "My New Action",
        "stdout": {}
    });

    // 1. Successful creation
    let resp = server
        .post("/actions")
        .await
        .bearer_auth("test-key")
        .json(&action_json)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "Failed to create action");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["action"]["id"], 1);
    assert_eq!(body["action"]["name"], "My New Action");

    // 2. Name conflict
    let resp = server
        .post("/actions")
        .await
        .bearer_auth("test-key")
        .json(&action_json)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409, "Should return conflict for duplicate name");

    // 3. Validation error
    let invalid_action_json = serde_json::json!({
        "name": "My Invalid Action",
        "kafka": { "brokers": "" } // Invalid: empty brokers
    });
    let resp = server
        .post("/actions")
        .await
        .bearer_auth("test-key")
        .json(&invalid_action_json)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 422, "Should return unprocessable entity for invalid config");

    server.cleanup();
}
