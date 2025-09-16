use std::{collections::HashSet, sync::Arc};

use alloy::primitives::{Address, B256};

use crate::{
    abi::AbiService,
    config::RhaiConfig,
    engine::rhai::RhaiCompiler,
    monitor::{InterestRegistry, MonitorAssetState, MonitorManager},
};

/// Creates a test `MonitorManager` with the specified log addresses and global
/// topics.
pub fn create_test_monitor_manager(
    log_addresses: Vec<Address>,
    global_topics: Vec<B256>,
) -> Arc<MonitorManager> {
    let compiler = Arc::new(RhaiCompiler::new(RhaiConfig::default()));
    let abi_service = Arc::new(AbiService::new(Default::default()));
    let manager = MonitorManager::new(vec![], compiler, abi_service);
    let interest_registry = InterestRegistry {
        log_addresses: Arc::new(log_addresses.into_iter().collect()),
        calldata_addresses: Arc::new(HashSet::new()),
        global_event_signatures: Arc::new(global_topics.into_iter().collect()),
    };
    let state = MonitorAssetState { interest_registry, ..Default::default() };
    manager.state.store(Arc::new(state));
    Arc::new(manager)
}
