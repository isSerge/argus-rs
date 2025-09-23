//! Integration tests for the MatchManager service

use std::{collections::HashMap, fs, sync::Arc, time::Duration};

use argus::{
    engine::{action_handler::ActionHandler, match_manager::MatchManager},
    http_client::HttpClientPool,
    models::{
        NotificationMessage,
        match_manager_state::{AggregationState, ThrottleState},
        notifier::{
            AggregationPolicy, DiscordConfig, NotifierConfig, NotifierPolicy, NotifierTypeConfig,
            ThrottlePolicy,
        },
    },
    notification::NotificationService,
    persistence::{sqlite::SqliteStateRepository, traits::GenericStateRepository},
    test_helpers::{
        MonitorBuilder, create_test_match_manager_with_repo, create_test_monitor_manager,
    },
};
use argus_models::{config::ActionConfig, monitor_match::MonitorMatch};
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

fn create_monitor_match(monitor_name: &str, notifier_name: &str) -> MonitorMatch {
    MonitorMatch::new_tx_match(
        1,
        monitor_name.to_string(),
        notifier_name.to_string(),
        123,
        Default::default(),
        json!({ "key": "value" }),
    )
}

#[tokio::test]
async fn test_aggregation_policy_dispatches_summary_after_window() {
    let mut server = mockito::Server::new_async().await;
    let notifier_name = "test_aggregator".to_string();
    let monitor_name = "Test Monitor".to_string();
    let aggregation_window_secs = 1;

    let aggregation_policy = AggregationPolicy {
        window_secs: Duration::from_secs(aggregation_window_secs),
        template: NotificationMessage {
            title: "Aggregated Alert: {{ monitor_name }}".to_string(),
            body: "Detected {{ matches | length }} events.".to_string(),
        },
    };

    let notifier_config = NotifierConfig {
        name: notifier_name.clone(),
        config: NotifierTypeConfig::Discord(DiscordConfig {
            discord_url: server.url(),
            message: NotificationMessage {
                title: "Single Alert".to_string(),
                body: "Single event body".to_string(),
            },
            retry_policy: Default::default(),
        }),
        policy: Some(NotifierPolicy::Aggregation(aggregation_policy.clone())),
    };
    let mut notifiers = HashMap::new();
    notifiers.insert(notifier_name.clone(), notifier_config);

    let state_repo = Arc::new(setup_db().await);
    let match_manager =
        create_test_match_manager_with_repo(Arc::new(notifiers), state_repo.clone());

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
    let match1 = create_monitor_match(&monitor_name, &notifier_name);
    let match2 = create_monitor_match(&monitor_name, &notifier_name);

    match_manager.process_match(match1).await.unwrap();
    match_manager.process_match(match2).await.unwrap();

    // Spawn the aggregation dispatcher in the background
    let dispatcher_match_manager = Arc::new(match_manager);
    tokio::spawn(async move {
        dispatcher_match_manager.run_aggregation_dispatcher(Duration::from_millis(100)).await;
    });

    // Wait for the aggregation window to pass and dispatcher to run
    sleep(Duration::from_secs(aggregation_window_secs + 1)).await;

    // Assert that the aggregated notification was sent
    mock.assert();

    // Verify that the aggregation state is cleared from the repository
    let state_key = format!("aggregation_state:{}", notifier_name);
    let cleared_state = state_repo.get_json_state::<AggregationState>(&state_key).await.unwrap();
    assert!(cleared_state.is_some());
    assert!(cleared_state.unwrap().matches.is_empty());
}

#[tokio::test]
async fn test_throttle_policy_limits_notifications() {
    let mut server = mockito::Server::new_async().await;
    let notifier_name = "test_throttler".to_string();
    let monitor_name = "Test Monitor".to_string();
    let max_count = 2;
    let time_window_secs = 1;

    let throttle_policy =
        ThrottlePolicy { max_count, time_window_secs: Duration::from_secs(time_window_secs) };

    let notifier_config = NotifierConfig {
        name: notifier_name.clone(),
        config: NotifierTypeConfig::Discord(DiscordConfig {
            discord_url: server.url(),
            message: NotificationMessage {
                title: "Throttled Alert".to_string(),
                body: "Throttled event body".to_string(),
            },
            retry_policy: Default::default(),
        }),
        policy: Some(NotifierPolicy::Throttle(throttle_policy.clone())),
    };
    let mut notifiers = HashMap::new();
    notifiers.insert(notifier_name.clone(), notifier_config);

    let state_repo = Arc::new(setup_db().await);
    let match_manager =
        create_test_match_manager_with_repo(Arc::new(notifiers), state_repo.clone());

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
        let monitor_match = create_monitor_match(&monitor_name, &notifier_name);
        match_manager.process_match(monitor_match).await.unwrap();
    }

    // Wait for a short period to ensure all process_match calls complete
    sleep(Duration::from_millis(100)).await;

    // Assert that only `max_count` notifications were sent
    mock.assert();

    // Verify that the throttle state is correctly updated in the repository
    let state_key = format!("throttle_state:{}", notifier_name);
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
    let monitor_match = create_monitor_match(&monitor_name, &notifier_name);
    match_manager.process_match(monitor_match).await.unwrap();

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
    let notifier_name = "test_no_policy".to_string();
    let monitor_name = "Test Monitor".to_string();

    let notifier_config = NotifierConfig {
        name: notifier_name.clone(),
        config: NotifierTypeConfig::Discord(DiscordConfig {
            discord_url: server.url(),
            message: NotificationMessage {
                title: "Simple Alert".to_string(),
                body: "Simple event body".to_string(),
            },
            retry_policy: Default::default(),
        }),
        policy: None, // No policy
    };
    let mut notifiers = HashMap::new();
    notifiers.insert(notifier_name.clone(), notifier_config);

    let state_repo = Arc::new(setup_db().await);
    let match_manager =
        create_test_match_manager_with_repo(Arc::new(notifiers), state_repo.clone());

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
    let match1 = create_monitor_match(&monitor_name, &notifier_name);
    let match2 = create_monitor_match(&monitor_name, &notifier_name);

    match_manager.process_match(match1).await.unwrap();
    match_manager.process_match(match2).await.unwrap();

    // Wait for a short period to ensure notifications are sent
    sleep(Duration::from_millis(100)).await;

    // Assert that both notifications were sent
    mock.assert();
}

#[tokio::test]
async fn test_throttle_policy_shared_across_monitors() {
    let mut server = mockito::Server::new_async().await;
    let notifier_name = "shared_throttler".to_string();
    let monitor_name1 = "Monitor A".to_string();
    let monitor_name2 = "Monitor B".to_string();
    let max_count = 3;
    let time_window_secs = 2;

    let throttle_policy =
        ThrottlePolicy { max_count, time_window_secs: Duration::from_secs(time_window_secs) };

    let notifier_config = NotifierConfig {
        name: notifier_name.clone(),
        config: NotifierTypeConfig::Discord(DiscordConfig {
            discord_url: server.url(),
            message: NotificationMessage {
                title: "Shared Throttle Alert".to_string(),
                body: "Event body".to_string(),
            },
            retry_policy: Default::default(),
        }),
        policy: Some(NotifierPolicy::Throttle(throttle_policy.clone())),
    };
    let mut notifiers = HashMap::new();
    notifiers.insert(notifier_name.clone(), notifier_config);

    let state_repo = Arc::new(setup_db().await);
    let match_manager =
        create_test_match_manager_with_repo(Arc::new(notifiers), state_repo.clone());

    let mock = server
        .mock("POST", "/")
        .with_status(200)
        .with_body(r#"{"content":"*Shared Throttle Alert*\n\nEvent body"}"#)
        .expect(max_count as usize)
        .create_async()
        .await;

    // Send matches from two different monitors, exceeding the throttle limit
    let match1 = create_monitor_match(&monitor_name1, &notifier_name);
    let match2 = create_monitor_match(&monitor_name2, &notifier_name);
    let match3 = create_monitor_match(&monitor_name1, &notifier_name);
    let match4 = create_monitor_match(&monitor_name2, &notifier_name);

    match_manager.process_match(match1).await.unwrap();
    match_manager.process_match(match2).await.unwrap();
    match_manager.process_match(match3).await.unwrap();
    match_manager.process_match(match4).await.unwrap();

    sleep(Duration::from_millis(200)).await;

    // Assert that only `max_count` notifications were sent
    mock.assert();

    // Verify the throttle state
    let state_key = format!("throttle_state:{}", notifier_name);
    let throttle_state =
        state_repo.get_json_state::<ThrottleState>(&state_key).await.unwrap().unwrap();
    assert_eq!(throttle_state.count, max_count);
}

#[tokio::test]
async fn test_aggregation_state_persistence_on_restart() {
    let mut server = mockito::Server::new_async().await;
    let notifier_name = "persistent_aggregator".to_string();
    let monitor_name = "Persistent Monitor".to_string();
    let aggregation_window_secs = 1;

    let aggregation_policy = AggregationPolicy {
        window_secs: Duration::from_secs(aggregation_window_secs),
        template: NotificationMessage {
            title: "Persistent Aggregated Alert".to_string(),
            body: "Detected {{ matches | length }} persistent events.".to_string(),
        },
    };

    let notifier_config = NotifierConfig {
        name: notifier_name.clone(),
        config: NotifierTypeConfig::Discord(DiscordConfig {
            discord_url: server.url(),
            message: Default::default(),
            retry_policy: Default::default(),
        }),
        policy: Some(NotifierPolicy::Aggregation(aggregation_policy.clone())),
    };
    let mut notifiers = HashMap::new();
    notifiers.insert(notifier_name.clone(), notifier_config);
    let notifiers_arc = Arc::new(notifiers);

    let state_repo = Arc::new(setup_db().await);

    // --- First run: process matches and store state ---
    let match_manager1 =
        create_test_match_manager_with_repo(notifiers_arc.clone(), state_repo.clone());
    let match1 = create_monitor_match(&monitor_name, &notifier_name);
    let match2 = create_monitor_match(&monitor_name, &notifier_name);
    match_manager1.process_match(match1).await.unwrap();
    match_manager1.process_match(match2).await.unwrap();

    // Verify state is persisted
    let state_key = format!("aggregation_state:{}", notifier_name);
    let saved_state =
        state_repo.get_json_state::<AggregationState>(&state_key).await.unwrap().unwrap();
    assert_eq!(saved_state.matches.len(), 2);

    // --- Simulate restart: create a new MatchManager with the same state ---
    let match_manager2 =
        create_test_match_manager_with_repo(notifiers_arc.clone(), state_repo.clone());

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
    let dispatcher_match_manager = Arc::new(match_manager2);
    tokio::spawn(async move {
        dispatcher_match_manager.run_aggregation_dispatcher(Duration::from_millis(100)).await;
    });

    sleep(Duration::from_secs(aggregation_window_secs + 1)).await;

    mock.assert();

    // Verify state is cleared after dispatch
    let cleared_state =
        state_repo.get_json_state::<AggregationState>(&state_key).await.unwrap().unwrap();
    assert!(cleared_state.matches.is_empty());
}

#[tokio::test]
async fn test_process_match_with_invalid_notifier() {
    let state_repo = Arc::new(setup_db().await);
    // No notifiers configured
    let match_manager =
        create_test_match_manager_with_repo(Arc::new(HashMap::new()), state_repo.clone());

    let monitor_match = create_monitor_match("any_monitor", "non_existent_notifier");

    let result = match_manager.process_match(monitor_match).await;

    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.to_string().contains("Notifier 'non_existent_notifier' not found"));
    }
}

#[tokio::test]
async fn test_process_match_with_action_handler_returns_modified_match() {
    let mut server = mockito::Server::new_async().await;
    let monitor_name = "test_monitor".to_string();
    let notifier_name = "default_notifier".to_string();
    let action_name = "modify_action".to_string();

    let notifier_config = NotifierConfig {
        name: notifier_name.clone(),
        config: NotifierTypeConfig::Discord(DiscordConfig {
            discord_url: server.url(),
            message: NotificationMessage {
                title: "Title".to_string(),
                body: "Monitor name modified by action: {{ monitor_name }}".to_string(),
            },
            retry_policy: Default::default(),
        }),
        policy: None, // No policy
    };
    let mut notifiers = HashMap::new();
    notifiers.insert(notifier_name.clone(), notifier_config);

    let notifiers_arc = Arc::new(notifiers);

    let temp_dir = tempfile::tempdir().unwrap();
    let action_file_path = temp_dir.path().join("test_action.js");
    // This action modifies the monitor_name in the match
    fs::write(
        &action_file_path,
        r#"
        match.monitor_name = "test_monitor_modified_by_action";
    "#,
    )
    .unwrap();

    let action_config = ActionConfig { name: action_name.clone(), file: action_file_path };

    let mut actions = HashMap::new();
    actions.insert(action_config.name.clone(), action_config);

    let monitor = MonitorBuilder::new()
        .name(&monitor_name)
        .notifiers(vec![notifier_name.clone()])
        .on_match(vec![action_name.clone()])
        .build();

    let monitor_manager = create_test_monitor_manager(vec![monitor]);
    let action_handler = ActionHandler::new(Arc::new(actions), monitor_manager.clone());

    let state_repo = Arc::new(setup_db().await);
    let notification_service = Arc::new(NotificationService::new(
        notifiers_arc.clone(),
        Arc::new(HttpClientPool::default()),
    ));

    let match_manager = Arc::new(MatchManager::new(
        notification_service,
        state_repo.clone(),
        notifiers_arc.clone(),
        Some(Arc::new(action_handler)),
    ));

    // Mock the notification endpoint
    let mock = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"content":"*Title*\n\nMonitor name modified by action: test_monitor_modified_by_action"}"#)
        .expect(1)
        .create_async()
        .await;

    let monitor_match = create_monitor_match(&monitor_name, &notifier_name);

    let result = match_manager.process_match(monitor_match.clone()).await;

    assert!(result.is_ok(), "Processing match failed: {:?}", result.err());

    // Wait for a short period to ensure notifications are sent
    sleep(Duration::from_millis(100)).await;

    // Assert that the notification was sent
    mock.assert();
}
