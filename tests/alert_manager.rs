//! Integration tests for the AlertManager service

use std::{collections::HashMap, sync::Arc, time::Duration};

use argus::{
    engine::alert_manager::AlertManager,
    http_client::HttpClientPool,
    models::{
        NotificationMessage,
        action::{ActionConfig, ActionPolicy, AggregationPolicy, ThrottlePolicy},
        alert_manager_state::{AggregationState, ThrottleState},
        monitor_match::MonitorMatch,
    },
    notification::NotificationService,
    persistence::{sqlite::SqliteStateRepository, traits::KeyValueStore},
    test_helpers::ActionBuilder,
};
use mockito;
use serde_json::json;
use tokio::time::sleep;

async fn setup_db() -> SqliteStateRepository {
    let repo = SqliteStateRepository::new("sqlite::memory:")
        .await
        .expect("Failed to set up in-memory database");
    repo.run_migrations().await.expect("Failed to run migrations");
    repo
}

fn create_monitor_match(monitor_name: &str, action_name: &str) -> MonitorMatch {
    MonitorMatch::new_tx_match(
        1,
        monitor_name.to_string(),
        action_name.to_string(),
        123,
        Default::default(),
        json!({ "key": "value" }),
    )
}

fn create_alert_manager(
    actions: HashMap<String, ActionConfig>,
    state_repo: Arc<SqliteStateRepository>,
) -> AlertManager<SqliteStateRepository> {
    let actions_arc = Arc::new(actions);
    let notification_service = Arc::new(NotificationService::new(
        actions_arc.clone(),
        Arc::new(HttpClientPool::default()),
    ));
    AlertManager::new(notification_service, state_repo, actions_arc)
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

    let state_repo = Arc::new(setup_db().await);
    let alert_manager = create_alert_manager(actions, state_repo.clone());

    // Mock the aggregated notification
    let mock = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"content":"*Aggregated Alert: {}*\n\nDetected 2 events."}}"#,
            monitor_name
        ))
        .expect(1) // Expect only one aggregated notification
        .create_async()
        .await;

    // Send two matches within the aggregation window
    let match1 = create_monitor_match(&monitor_name, &action_name);
    let match2 = create_monitor_match(&monitor_name, &action_name);

    alert_manager.process_match(&match1).await.unwrap();
    alert_manager.process_match(&match2).await.unwrap();

    // Spawn the aggregation dispatcher in the background
    let dispatcher_alert_manager = Arc::new(alert_manager);
    tokio::spawn(async move {
        dispatcher_alert_manager.run_aggregation_dispatcher(Duration::from_millis(100)).await;
    });

    // Wait for the aggregation window to pass and dispatcher to run
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
    let alert_manager = create_alert_manager(actions, state_repo.clone());

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

    // Wait for a short period to ensure all process_match calls complete
    sleep(Duration::from_millis(100)).await;

    // Assert that only `max_count` notifications were sent
    mock.assert();

    // Verify that the throttle state is correctly updated in the repository
    let state_key = format!("throttle_state:{}", action_name);
    let throttle_state =
        state_repo.get_json_state::<ThrottleState>(&state_key).await.unwrap().unwrap();
    assert_eq!(throttle_state.count, max_count);

    // Wait for the throttle window to expire
    sleep(Duration::from_secs(time_window_secs + 1)).await;

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

    // Assert that another notification is sent after the window reset
    sleep(Duration::from_millis(100)).await;
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
    let alert_manager = create_alert_manager(actions, state_repo.clone());

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

    // Wait for a short period to ensure notifications are sent
    sleep(Duration::from_millis(100)).await;

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
    let alert_manager = create_alert_manager(actions, state_repo.clone());

    let mock = server
        .mock("POST", "/")
        .with_status(200)
        .with_body(r#"{"content":"*Shared Throttle Alert*\n\nEvent body"}"#)
        .expect(max_count as usize)
        .create_async()
        .await;

    // Send matches from two different monitors, exceeding the throttle limit
    let match1 = create_monitor_match(&monitor_name1, &action_name);
    let match2 = create_monitor_match(&monitor_name2, &action_name);
    let match3 = create_monitor_match(&monitor_name1, &action_name);
    let match4 = create_monitor_match(&monitor_name2, &action_name);

    alert_manager.process_match(&match1).await.unwrap();
    alert_manager.process_match(&match2).await.unwrap();
    alert_manager.process_match(&match3).await.unwrap();
    alert_manager.process_match(&match4).await.unwrap();

    sleep(Duration::from_millis(200)).await;

    // Assert that only `max_count` notifications were sent
    mock.assert();

    // Verify the throttle state
    let state_key = format!("throttle_state:{}", action_name);
    let throttle_state =
        state_repo.get_json_state::<ThrottleState>(&state_key).await.unwrap().unwrap();
    assert_eq!(throttle_state.count, max_count);
}

#[tokio::test]
async fn test_aggregation_state_persistence_on_restart() {
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
    let actions_arc = Arc::new(actions);

    let state_repo = Arc::new(setup_db().await);

    // --- First run: process matches and store state ---
    let alert_manager1 = create_alert_manager(actions_arc.as_ref().clone(), state_repo.clone());
    let match1 = create_monitor_match(&monitor_name, &action_name);
    let match2 = create_monitor_match(&monitor_name, &action_name);
    alert_manager1.process_match(&match1).await.unwrap();
    alert_manager1.process_match(&match2).await.unwrap();

    // Verify state is persisted
    let state_key = format!("aggregation_state:{}", action_name);
    let saved_state =
        state_repo.get_json_state::<AggregationState>(&state_key).await.unwrap().unwrap();
    assert_eq!(saved_state.matches.len(), 2);

    // --- Simulate restart: create a new AlertManager with the same state ---
    let alert_manager2 = create_alert_manager(actions_arc.as_ref().clone(), state_repo.clone());

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
    let alert_manager = create_alert_manager(HashMap::new(), state_repo.clone());

    let monitor_match = create_monitor_match("any_monitor", "non_existent_action");

    let result = alert_manager.process_match(&monitor_match).await;

    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.to_string().contains("Action 'non_existent_action' not found"));
    }
}
