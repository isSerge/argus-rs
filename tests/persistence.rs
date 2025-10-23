//! Integration tests for the persistence layer

use argus::{
    models::{action::ActionConfig, monitor::MonitorConfig},
    persistence::{sqlite::SqliteStateRepository, traits::AppRepository},
    test_helpers::ActionBuilder,
};

async fn setup_db() -> SqliteStateRepository {
    let repo = SqliteStateRepository::new("sqlite::memory:")
        .await
        .expect("Failed to set up in-memory database");
    repo.run_migrations().await.expect("Failed to run migrations");
    repo
}

fn create_test_monitor(name: &str, network: &str) -> MonitorConfig {
    MonitorConfig::from_config(
        name.to_string(),
        network.to_string(),
        Some("0x123".to_string()),
        Some("test".to_string()),
        "log.name == \"Test\"".to_string(),
        vec![],
    )
}

fn create_test_action(name: &str) -> ActionConfig {
    ActionBuilder::new(name).discord_config("https://discord.com/api/webhooks/test").build()
}

#[tokio::test]
async fn test_monitor_lifecycle() {
    let repo = setup_db().await;
    let network_id = "ethereum";

    // 1. Initially, no monitors should exist
    let initial_monitors = repo.get_monitors(network_id).await.unwrap();
    assert!(initial_monitors.is_empty());

    // 2. Add monitors
    let monitors_to_add = vec![
        create_test_monitor("Monitor 1", network_id),
        create_test_monitor("Monitor 2", network_id),
    ];
    repo.add_monitors(network_id, monitors_to_add.clone()).await.unwrap();

    // 3. Get monitors and verify they were added
    let stored_monitors = repo.get_monitors(network_id).await.unwrap();
    assert_eq!(stored_monitors.len(), 2);
    assert_eq!(stored_monitors[0].name, "Monitor 1");
    assert_eq!(stored_monitors[1].name, "Monitor 2");

    // 4. Clear monitors
    repo.clear_monitors(network_id).await.unwrap();
    let cleared_monitors = repo.get_monitors(network_id).await.unwrap();
    assert!(cleared_monitors.is_empty());
}

#[tokio::test]
async fn test_action_lifecycle() {
    let repo = setup_db().await;
    let network_id = "ethereum";

    // 1. Initially, no actions should exist
    let initial_actions = repo.get_actions(network_id).await.unwrap();
    assert!(initial_actions.is_empty());

    // 2. Add actions
    let actions_to_add = vec![create_test_action("Action 1"), create_test_action("Action 2")];
    repo.create_action(network_id, actions_to_add[0].clone()).await.unwrap();
    repo.create_action(network_id, actions_to_add[1].clone()).await.unwrap();

    // 3. Get actions and verify they were added
    let stored_actions = repo.get_actions(network_id).await.unwrap();
    assert_eq!(stored_actions.len(), 2);
    assert_eq!(stored_actions[0].name, "Action 1");
    assert_eq!(stored_actions[1].name, "Action 2");

    // 4. Clear actions
    repo.clear_actions(network_id).await.unwrap();
    let cleared_actions = repo.get_actions(network_id).await.unwrap();
    assert!(cleared_actions.is_empty());
}

#[tokio::test]
async fn test_processed_block_management() {
    let repo = setup_db().await;
    let network_id = "ethereum";

    // 1. Initially, last processed block should be None
    let initial_block = repo.get_last_processed_block(network_id).await.unwrap();
    assert!(initial_block.is_none());

    // 2. Set and get the last processed block
    repo.set_last_processed_block(network_id, 12345).await.unwrap();
    let retrieved_block = repo.get_last_processed_block(network_id).await.unwrap();
    assert_eq!(retrieved_block, Some(12345));

    // 3. Update the last processed block
    repo.set_last_processed_block(network_id, 54321).await.unwrap();
    let updated_block = repo.get_last_processed_block(network_id).await.unwrap();
    assert_eq!(updated_block, Some(54321));
}

#[tokio::test]
async fn test_network_isolation() {
    let repo = setup_db().await;
    let eth_network = "ethereum";
    let poly_network = "polygon";

    // Add monitors and actions to both networks
    repo.add_monitors(eth_network, vec![create_test_monitor("ETH Monitor", eth_network)])
        .await
        .unwrap();
    repo.add_monitors(poly_network, vec![create_test_monitor("Polygon Monitor", poly_network)])
        .await
        .unwrap();
    repo.create_action(eth_network, create_test_action("ETH Action")).await.unwrap();
    repo.create_action(poly_network, create_test_action("Polygon Action")).await.unwrap();

    // Verify data for Ethereum
    let eth_monitors = repo.get_monitors(eth_network).await.unwrap();
    let eth_actions = repo.get_actions(eth_network).await.unwrap();
    assert_eq!(eth_monitors.len(), 1);
    assert_eq!(eth_monitors[0].name, "ETH Monitor");
    assert_eq!(eth_actions.len(), 1);
    assert_eq!(eth_actions[0].name, "ETH Action");

    // Verify data for Polygon
    let poly_monitors = repo.get_monitors(poly_network).await.unwrap();
    let poly_actions = repo.get_actions(poly_network).await.unwrap();
    assert_eq!(poly_monitors.len(), 1);
    assert_eq!(poly_monitors[0].name, "Polygon Monitor");
    assert_eq!(poly_actions.len(), 1);
    assert_eq!(poly_actions[0].name, "Polygon Action");

    // Clear Ethereum and verify it doesn't affect Polygon
    repo.clear_monitors(eth_network).await.unwrap();
    repo.clear_actions(eth_network).await.unwrap();

    assert!(repo.get_monitors(eth_network).await.unwrap().is_empty());
    assert!(repo.get_actions(eth_network).await.unwrap().is_empty());
    assert_eq!(repo.get_monitors(poly_network).await.unwrap().len(), 1);
    assert_eq!(repo.get_actions(poly_network).await.unwrap().len(), 1);
}
