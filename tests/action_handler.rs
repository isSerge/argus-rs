//! Integration tests for the ActionHandler service

use std::{collections::HashMap, io::Write, sync::Arc};

use alloy::primitives::TxHash;
use argus::{
    config::{ActionConfig, RhaiConfig},
    engine::{action_handler::ActionHandler, rhai::RhaiCompiler},
    models::{
        monitor::Monitor,
        monitor_match::{MatchData, MonitorMatch, TransactionMatchData},
    },
    monitor::MonitorManager,
    test_helpers::{self, MonitorBuilder},
};
use serde_json::json;
use tempfile::{TempDir, tempdir};

fn create_test_monitor_manager(monitors: Vec<Monitor>) -> Arc<MonitorManager> {
    let rhai_compiler = Arc::new(RhaiCompiler::new(RhaiConfig::default()));
    let temp_dir = tempdir().unwrap();
    let (abi_service, _) = test_helpers::create_test_abi_service(&temp_dir, &[]);
    let manager = MonitorManager::new(monitors, rhai_compiler, abi_service);
    Arc::new(manager)
}

fn create_test_monitor_match(monitor_id: i64) -> MonitorMatch {
    MonitorMatch {
        monitor_id,
        monitor_name: "Test Monitor".to_string(),
        notifier_name: "test-notifier".to_string(),
        block_number: 123,
        transaction_hash: TxHash::default(),
        match_data: MatchData::Transaction(TransactionMatchData { details: json!({"foo": "bar"}) }),
    }
}

fn create_test_action_file(dir: &TempDir, name: &str, content: &str) -> (String, ActionConfig) {
    let file_path = dir.path().join(name);
    let mut file = std::fs::File::create(&file_path).unwrap();
    writeln!(file, "{}", content).unwrap();
    let action_name = name.split('.').next().unwrap().to_string();
    (action_name.clone(), ActionConfig { name: action_name, file: file_path })
}

#[tokio::test]
async fn execute_no_actions() {
    let monitor = MonitorBuilder::new().id(1).on_match(vec![]).build();
    let monitor_manager = create_test_monitor_manager(vec![monitor]);
    let actions = Arc::new(HashMap::new());
    let handler = ActionHandler::new(actions, monitor_manager).await.unwrap();
    let monitor_match = create_test_monitor_match(1);

    let result = handler.execute(monitor_match.clone()).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), monitor_match);
}

#[tokio::test]
async fn execute_single_action_success() {
    let temp_dir = tempdir().unwrap();
    let (action_name, action_config) =
        create_test_action_file(&temp_dir, "modify.js", "match.monitor_name = 'Modified';");
    let monitor = MonitorBuilder::new().id(1).on_match(vec![action_name]).build();
    let monitor_manager = create_test_monitor_manager(vec![monitor]);
    let actions = Arc::new(HashMap::from([(action_config.name.clone(), action_config)]));
    let handler = ActionHandler::new(actions, monitor_manager).await.unwrap();
    let monitor_match = create_test_monitor_match(1);

    let result = handler.execute(monitor_match.clone()).await;
    assert!(result.is_ok());
    let modified_match = result.unwrap();
    assert_eq!(modified_match.monitor_name, "Modified");
}

#[tokio::test]
async fn execute_multiple_actions_success() {
    let temp_dir = tempdir().unwrap();
    let (action1_name, action1_config) = create_test_action_file(
        &temp_dir,
        "action1.js",
        "match.monitor_name = 'Action1 Modified';",
    );
    let (action2_name, action2_config) =
        create_test_action_file(&temp_dir, "action2.js", "match.block_number = 456;");
    let monitor = MonitorBuilder::new().id(1).on_match(vec![action1_name, action2_name]).build();
    let monitor_manager = create_test_monitor_manager(vec![monitor]);
    let actions = Arc::new(HashMap::from([
        (action1_config.name.clone(), action1_config),
        (action2_config.name.clone(), action2_config),
    ]));
    let handler = ActionHandler::new(actions, monitor_manager).await.unwrap();
    let monitor_match = create_test_monitor_match(1);

    let result = handler.execute(monitor_match.clone()).await;
    assert!(result.is_ok());
    let modified_match = result.unwrap();
    assert_eq!(modified_match.monitor_name, "Action1 Modified");
    assert_eq!(modified_match.block_number, 456);
}

#[tokio::test]
async fn execute_action_not_found() {
    let monitor =
        MonitorBuilder::new().id(1).on_match(vec!["non_existent_action".to_string()]).build();
    let monitor_manager = create_test_monitor_manager(vec![monitor]);
    let actions = Arc::new(HashMap::new());
    let handler = ActionHandler::new(actions, monitor_manager).await.unwrap();
    let monitor_match = create_test_monitor_match(1);

    let result = handler.execute(monitor_match.clone()).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), monitor_match);
}

#[tokio::test]
async fn execute_action_execution_error() {
    let temp_dir = tempdir().unwrap();
    let (action_name, action_config) =
        create_test_action_file(&temp_dir, "error.js", "throw new Error('test error');");
    let monitor = MonitorBuilder::new().id(1).on_match(vec![action_name]).build();
    let monitor_manager = create_test_monitor_manager(vec![monitor]);
    let actions = Arc::new(HashMap::from([(action_config.name.clone(), action_config)]));
    let handler = ActionHandler::new(actions, monitor_manager).await.unwrap();
    let monitor_match = create_test_monitor_match(1);

    let result = handler.execute(monitor_match.clone()).await;
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.to_string().contains("test error"));
    }
}

#[tokio::test]
async fn execute_monitor_not_found() {
    let monitor_manager = create_test_monitor_manager(vec![]);
    let actions = Arc::new(HashMap::new());
    let handler = ActionHandler::new(actions, monitor_manager).await.unwrap();
    let monitor_match = create_test_monitor_match(999); // Non-existent monitor ID

    let result = handler.execute(monitor_match.clone()).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), monitor_match);
}
