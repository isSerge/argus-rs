use std::sync::Arc;

use alloy::primitives::Address;
pub use argus_abi::test_utils::erc20_abi_json;
use argus_abi::{AbiService, repository::AbiRepository};
use argus_core::{
    config::RhaiConfig,
    models::{NetworkId, action::ActionConfig},
    persistence::traits::AppRepository,
};
use argus_dispatch::template::TemplateService;
use argus_rhai::{RhaiCompiler, RhaiScriptValidator};
use argus_store::SqliteStateRepository;

use crate::MonitorValidator;

mod monitor_manager;
pub use monitor_manager::create_test_monitor_manager;

/// Creates a test `MonitorValidator` with optional preloaded ABI.
pub async fn create_monitor_validator(
    actions: &[ActionConfig],
    abi_to_preload: Option<(Address, &'static str, &'static str)>,
) -> MonitorValidator {
    let config = RhaiConfig::default();
    let compiler = Arc::new(RhaiCompiler::new(config));
    let script_validator = RhaiScriptValidator::new(compiler);
    let template_service = Arc::new(TemplateService::new());

    let repo = SqliteStateRepository::new("sqlite::memory:")
        .await
        .expect("Failed to connect to in-memory db");
    repo.run_migrations().await.expect("Failed to run migrations");

    if let Some((_, abi_name, abi_json_str)) = &abi_to_preload {
        repo.create_abi(abi_name, abi_json_str).await.unwrap();
    }
    let abi_repository = Arc::new(AbiRepository::new(Arc::new(repo)).await.unwrap());

    let abi_service = Arc::new(AbiService::new(Arc::clone(&abi_repository)));
    if let Some((address, abi_name, _)) = abi_to_preload {
        abi_service.link_abi(address, abi_name).unwrap();
    }

    let actions_arc = Arc::new(actions.to_vec());
    MonitorValidator::new(
        script_validator,
        abi_service,
        template_service,
        NetworkId::default(),
        actions_arc,
    )
}
