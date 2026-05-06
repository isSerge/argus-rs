use std::sync::Arc;

use argus_core::{config::RhaiConfig, models::monitor::Monitor};

use crate::{
    engine::rhai::RhaiCompiler,
    monitor::MonitorManager,
    test_utils::{create_test_abi_service, erc20_abi_json},
};

/// Creates a test `MonitorManager` with the given monitors.
pub async fn create_test_monitor_manager(monitors: Vec<Monitor>) -> Arc<MonitorManager> {
    let compiler = Arc::new(RhaiCompiler::new(RhaiConfig::default()));
    let (abi_service, _) = create_test_abi_service(&[("erc20", erc20_abi_json())]).await;
    let manager = MonitorManager::new(monitors, compiler, abi_service);
    Arc::new(manager)
}
