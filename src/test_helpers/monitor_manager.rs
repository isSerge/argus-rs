use std::sync::Arc;

use tempfile::tempdir;

use crate::{
    config::RhaiConfig,
    engine::rhai::RhaiCompiler,
    models::monitor::Monitor,
    monitor::MonitorManager,
    test_helpers::{create_test_abi_service, erc20_abi_json},
};

/// Creates a test `MonitorManager` with the given monitors.
pub fn create_test_monitor_manager(monitors: Vec<Monitor>) -> Arc<MonitorManager> {
    let temp_dir = tempdir().unwrap();
    let compiler = Arc::new(RhaiCompiler::new(RhaiConfig::default()));
    // Create a simple ABI service with a sample ABI
    let (abi_service, _) = create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json())]);
    let manager = MonitorManager::new(monitors, compiler, abi_service);
    Arc::new(manager)
}
