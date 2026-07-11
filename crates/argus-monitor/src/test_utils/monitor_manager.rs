use std::sync::Arc;

use argus_abi::test_utils::{create_test_abi_service, erc20_abi_json};
use argus_core::{config::RhaiConfig, models::monitor::Monitor};

use crate::{manager::MonitorManager, test_utils::RhaiCompiler};

/// Creates a test `MonitorManager` with the given monitors.
pub async fn create_test_monitor_manager(monitors: Vec<Monitor>) -> Arc<MonitorManager> {
    let compiler = Arc::new(RhaiCompiler::new(RhaiConfig::default()));
    let abi_service = create_test_abi_service(&[("erc20", erc20_abi_json())]).await;
    let manager = MonitorManager::new(monitors, compiler, abi_service);
    Arc::new(manager)
}
