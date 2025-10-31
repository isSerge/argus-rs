use std::sync::Arc;

use alloy::primitives::Address;

use crate::{
    abi::{AbiService, repository::AbiRepository},
    action_dispatcher::template::TemplateService,
    config::RhaiConfig,
    engine::rhai::{RhaiCompiler, RhaiScriptValidator},
    models::action::ActionConfig,
    monitor::MonitorValidator,
    persistence::{SqliteStateRepository, traits::AppRepository},
};

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

    // Create and populate AbiRepository
    if let Some((_, abi_name, abi_json_str)) = &abi_to_preload {
        repo.create_abi(abi_name, abi_json_str).await.unwrap();
    }
    let abi_repository = Arc::new(AbiRepository::new(Arc::new(repo)).await.unwrap());

    // Create AbiService and link ABIs
    let abi_service = Arc::new(AbiService::new(Arc::clone(&abi_repository)));
    if let Some((address, abi_name, _)) = abi_to_preload {
        abi_service.link_abi(address, abi_name).unwrap();
    }

    let actions_arc = Arc::new(actions.to_vec());
    MonitorValidator::new(script_validator, abi_service, template_service, "testnet", actions_arc)
}
