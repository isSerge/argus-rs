//! Integration tests for the AlertManager service + Outbox Processor

use std::{collections::HashMap, sync::Arc, time::Duration};

use argus::{
    action_dispatcher::ActionDispatcher,
    config::OutboxConfig,
    engine::{alert_manager::AlertManager, outbox_processor::OutboxProcessor},
    http_client::HttpClientPool,
    models::{
        NotificationMessage,
        action::{ActionConfig, ActionPolicy, AggregationPolicy, ThrottlePolicy},
        alert_manager_state::{AggregationState, ThrottleState},
        monitor_match::MonitorMatch,
    },
    persistence::{sqlite::SqliteStateRepository, traits::KeyValueStore},
    test_helpers::{ActionBuilder, create_test_tx_monitor_match},
};
use serde_json::json;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

async fn setup_db() -> SqliteStateRepository {
    let repo = SqliteStateRepository::new("sqlite::memory:")
        .await
        .expect("Failed to set up in-memory database");
    repo.run_migrations().await.expect("Failed to run migrations");
    repo
}

fn create_monitor_match(monitor_name: &str, action_name: &str) -> MonitorMatch {
    create_test_tx_monitor_match(monitor_name, action_name, json!({ "key": "value" }))
}

// Helper to build the dispatcher
async fn create_dispatcher(actions: HashMap<String, ActionConfig>) -> Arc<ActionDispatcher> {
    let actions_arc = Arc::new(actions);
    Arc::new(ActionDispatcher::new(actions_arc, Arc::new(HttpClientPool::default())).await.unwrap())
}

// Helper to build the AlertManager
async fn create_alert_manager(
    actions: HashMap<String, ActionConfig>,
    state_repo: Arc<SqliteStateRepository>,
) -> AlertManager<SqliteStateRepository> {
    let actions_arc = Arc::new(actions);
    AlertManager::new(state_repo, actions_arc)
}

// Helper to run the outbox processor in background
fn spawn_outbox_processor(repo: Arc<SqliteStateRepository>, dispatcher: Arc<ActionDispatcher>) {
    // Fast config for tests
    let config = OutboxConfig {
        batch_size: 10,
        concurrency: 1,
        poll_interval_ms: 50, // Fast polling
    };

    let processor = OutboxProcessor::new(repo, dispatcher, config);
    let cancellation_token = CancellationToken::new();
    tokio::spawn(async move {
        processor.run(cancellation_token).await;
    });
}

#[tokio::test]
async fn test_aggregation_policy_dispatches_summary_after_window() {
    let mut server = mockito::Server::new_async().await;
    let action_name = "test_aggregator".to_string();
    let monitor_name = "Test Monitor".to_string();
    let aggregation_window_secs = 1;

    let aggregation_policy = AggregationPolicy {
        window_secs: Duration::from_secs(aggregation_window_secs),
        template: NotificationMessage {
            title: "Aggregated Alert: {{ monitor_name }}".to_string(),
            body: "Detected {{ matches | length }} events.".to_string(),
        },
    };

    let action_config = ActionBuilder::new(&action_name)
        .discord_config(&server.url())
        .policy(ActionPolicy::Aggregation(aggregation_policy.clone()))
        .build();

    let mut actions = HashMap::new();
    actions.insert(action_name.clone(), action_config);

    // 1. Setup
    let state_repo = Arc::new(setup_db().await);
    let dispatcher = create_dispatcher(actions.clone()).await;
    let alert_manager = create_alert_manager(actions, state_repo.clone()).await;

    // 2. Start Outbox Processor (Consumer)
    spawn_outbox_processor(state_repo.clone(), dispatcher);

    // 3. Mock
    let mock = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"content":"*Aggregated Alert: {}*\n\nDetected 2 events."}}"#,
            monitor_name
        ))
        .expect(1)
        .create_async()
        .await;

    // 4. Generate Matches (Producer)
    let match1 = create_monitor_match(&monitor_name, &action_name);
    let match2 = create_monitor_match(&monitor_name, &action_name);

    alert_manager.process_match(&match1).await.unwrap();
    alert_manager.process_match(&match2).await.unwrap();

    // 5. Start Aggregation Dispatcher (Internal AlertManager timer)
    let dispatcher_alert_manager = Arc::new(alert_manager);
    tokio::spawn(async move {
        dispatcher_alert_manager.run_aggregation_dispatcher(Duration::from_millis(100)).await;
    });

    // 6. Wait
    // Need enough time for: Window Expiry -> DB Write -> Outbox Poll -> HTTP
    // Request
    sleep(Duration::from_secs(aggregation_window_secs + 1)).await;

    // Assert that the aggregated notification was sent
    mock.assert();

    // Verify that the aggregation state is cleared from the repository
    let state_key = format!("aggregation_state:{}", action_name);
    let cleared_state = state_repo.get_json_state::<AggregationState>(&state_key).await.unwrap();
    assert!(cleared_state.is_some());
    assert!(cleared_state.unwrap().matches.is_empty());
}

#[tokio::test]
async fn test_throttle_policy_limits_notifications() {
    let mut server = mockito::Server::new_async().await;
    let action_name = "test_throttler".to_string();
    let monitor_name = "Test Monitor".to_string();
    let max_count = 2;
    let time_window_secs = 1;

    let throttle_policy =
        ThrottlePolicy { max_count, time_window_secs: Duration::from_secs(time_window_secs) };

    let action_config = ActionBuilder::new(&action_name)
        .discord_config(&server.url())
        .policy(ActionPolicy::Throttle(throttle_policy.clone()))
        .build();

    let mut actions = HashMap::new();
    actions.insert(action_name.clone(), action_config);

    let state_repo = Arc::new(setup_db().await);
    let dispatcher = create_dispatcher(actions.clone()).await;
    let alert_manager = create_alert_manager(actions, state_repo.clone()).await;
    spawn_outbox_processor(state_repo.clone(), dispatcher);

    // Mock the throttled notification
    let mock = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"content":"*Throttled Alert*\n\nThrottled event body"}"#)
        .expect(max_count as usize) // Expect only `max_count` notifications
        .create_async()
        .await;

    // Send more matches than allowed by the throttle policy within the window
    for _ in 0..(max_count + 2) {
        let monitor_match = create_monitor_match(&monitor_name, &action_name);
        alert_manager.process_match(&monitor_match).await.unwrap();
    }

    // Wait for async processing
    sleep(Duration::from_millis(500)).await;

    // Assert that only `max_count` notifications were sent
    mock.assert();

    // Verify that the throttle state is correctly updated in the repository
    let state_key = format!("throttle_state:{}", action_name);
    let throttle_state =
        state_repo.get_json_state::<ThrottleState>(&state_key).await.unwrap().unwrap();
    assert_eq!(throttle_state.count, max_count);

    // --- Part 2: Window Reset ---

    // Wait for the throttle window to expire
    sleep(Duration::from_secs(time_window_secs + 1)).await;

    // Mock for the notification after the window reset
    let mock_after_reset = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"content":"*Throttled Alert*\n\nThrottled event body"}"#)
        .expect(1) // Expect one more notification
        .create_async()
        .await;

    // Send another match after the window expires
    let monitor_match = create_monitor_match(&monitor_name, &action_name);
    alert_manager.process_match(&monitor_match).await.unwrap();

    sleep(Duration::from_millis(500)).await;
    mock_after_reset.assert();

    let throttle_state_after_reset =
        state_repo.get_json_state::<ThrottleState>(&state_key).await.unwrap().unwrap();
    assert_eq!(throttle_state_after_reset.count, 1);
}

#[tokio::test]
async fn test_no_policy_sends_notification_per_match() {
    let mut server = mockito::Server::new_async().await;
    let action_name = "test_no_policy".to_string();
    let monitor_name = "Test Monitor".to_string();

    // No policy configured by default
    let action_config = ActionBuilder::new(&action_name).discord_config(&server.url()).build();

    let mut actions = HashMap::new();
    actions.insert(action_name.clone(), action_config);

    let state_repo = Arc::new(setup_db().await);
    let dispatcher = create_dispatcher(actions.clone()).await;
    let alert_manager = create_alert_manager(actions, state_repo.clone()).await;
    spawn_outbox_processor(state_repo.clone(), dispatcher);

    // Mock the notification endpoint
    let mock = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"content":"*Simple Alert*\n\nSimple event body"}"#)
        .expect(2) // Expect a notification for each match
        .create_async()
        .await;

    // Send two matches
    let match1 = create_monitor_match(&monitor_name, &action_name);
    let match2 = create_monitor_match(&monitor_name, &action_name);

    alert_manager.process_match(&match1).await.unwrap();
    alert_manager.process_match(&match2).await.unwrap();

    // Wait
    sleep(Duration::from_millis(500)).await;

    // Assert that both notifications were sent
    mock.assert();
}

#[tokio::test]
async fn test_throttle_policy_shared_across_monitors() {
    let mut server = mockito::Server::new_async().await;
    let action_name = "shared_throttler".to_string();
    let monitor_name1 = "Monitor A".to_string();
    let monitor_name2 = "Monitor B".to_string();
    let max_count = 3;
    let time_window_secs = 2;

    let throttle_policy =
        ThrottlePolicy { max_count, time_window_secs: Duration::from_secs(time_window_secs) };

    let action_config = ActionBuilder::new(&action_name)
        .discord_config(&server.url())
        .policy(ActionPolicy::Throttle(throttle_policy.clone()))
        .build();

    let mut actions = HashMap::new();
    actions.insert(action_name.clone(), action_config);

    let state_repo = Arc::new(setup_db().await);
    let dispatcher = create_dispatcher(actions.clone()).await;
    let alert_manager = create_alert_manager(actions, state_repo.clone()).await;
    spawn_outbox_processor(state_repo.clone(), dispatcher);

    let mock = server
        .mock("POST", "/")
        .with_status(200)
        .with_body(r#"{"content":"*Shared Throttle Alert*\n\nEvent body"}"#)
        .expect(max_count as usize)
        .create_async()
        .await;

    // Trigger: Send matches from two different monitors, exceeding the throttle
    // limit total
    let match1 = create_monitor_match(&monitor_name1, &action_name);
    let match2 = create_monitor_match(&monitor_name2, &action_name);
    let match3 = create_monitor_match(&monitor_name1, &action_name);
    let match4 = create_monitor_match(&monitor_name2, &action_name);

    alert_manager.process_match(&match1).await.unwrap();
    alert_manager.process_match(&match2).await.unwrap();
    alert_manager.process_match(&match3).await.unwrap();
    alert_manager.process_match(&match4).await.unwrap(); // This one should be throttled (dropped)

    // Wait for Outbox Processing
    sleep(Duration::from_millis(500)).await;

    // Assert that only `max_count` notifications were sent
    mock.assert();

    // Verify the throttle state
    let state_key = format!("throttle_state:{}", action_name);
    let throttle_state =
        state_repo.get_json_state::<ThrottleState>(&state_key).await.unwrap().unwrap();

    // The state tracks how many we ATTEMPTED to send, even if throttled
    // Logic:
    // 1. Count = 0 -> Send -> Count = 1
    // 2. Count = 1 -> Send -> Count = 2
    // 3. Count = 2 -> Send -> Count = 3
    // 4. Count = 3 -> Drop -> Count = 3 (The count stops incrementing once we hit
    //    limit in current impl)
    assert_eq!(throttle_state.count, max_count);
}

#[tokio::test]
async fn test_aggregation_state_persistence_on_restart() {
    // Here we verify that a new instance picks up pending state.

    let mut server = mockito::Server::new_async().await;
    let action_name = "persistent_aggregator".to_string();
    let monitor_name = "Persistent Monitor".to_string();
    let aggregation_window_secs = 1;

    let aggregation_policy = AggregationPolicy {
        window_secs: Duration::from_secs(aggregation_window_secs),
        template: NotificationMessage {
            title: "Persistent Aggregated Alert".to_string(),
            body: "Detected {{ matches | length }} persistent events.".to_string(),
        },
    };

    let action_config = ActionBuilder::new(&action_name)
        .discord_config(&server.url())
        .policy(ActionPolicy::Aggregation(aggregation_policy.clone()))
        .build();

    let mut actions = HashMap::new();
    actions.insert(action_name.clone(), action_config);

    let state_repo = Arc::new(setup_db().await);

    // --- Instance 1 ---
    let alert_manager1 = create_alert_manager(actions.clone(), state_repo.clone()).await;

    // Process matches
    let match1 = create_monitor_match(&monitor_name, &action_name);
    let match2 = create_monitor_match(&monitor_name, &action_name);
    alert_manager1.process_match(&match1).await.unwrap();
    alert_manager1.process_match(&match2).await.unwrap();

    // Verify state is persisted
    let state_key = format!("aggregation_state:{}", action_name);
    let saved_state =
        state_repo.get_json_state::<AggregationState>(&state_key).await.unwrap().unwrap();
    assert_eq!(saved_state.matches.len(), 2);

    // --- Instance 2 (Restart) ---
    // Simulate the restart by creating a new AlertManager on the same DB.
    let alert_manager2 = create_alert_manager(actions.clone(), state_repo.clone()).await;
    let dispatcher = create_dispatcher(actions.clone()).await;

    // Start Outbox
    spawn_outbox_processor(state_repo.clone(), dispatcher);

    let mock = server
        .mock("POST", "/")
        .with_status(200)
        .with_body(
            r#"{"content":"*Persistent Aggregated Alert*\n\nDetected 2 persistent events."}"#,
        )
        .expect(1)
        .create_async()
        .await;

    // Run the dispatcher on the new instance
    let dispatcher_alert_manager = Arc::new(alert_manager2);
    tokio::spawn(async move {
        dispatcher_alert_manager.run_aggregation_dispatcher(Duration::from_millis(100)).await;
    });

    sleep(Duration::from_secs(aggregation_window_secs + 1)).await;

    mock.assert();

    // Verify state is cleared after dispatch
    let cleared_state =
        state_repo.get_json_state::<AggregationState>(&state_key).await.unwrap().unwrap();
    assert!(cleared_state.matches.is_empty());
}

#[tokio::test]
async fn test_process_match_with_invalid_action() {
    let state_repo = Arc::new(setup_db().await);
    // No actions configured
    let alert_manager = create_alert_manager(HashMap::new(), state_repo.clone()).await;

    let monitor_match = create_monitor_match("any_monitor", "non_existent_action");

    let result = alert_manager.process_match(&monitor_match).await;

    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.to_string().contains("Action 'non_existent_action' not found"));
    }
}
