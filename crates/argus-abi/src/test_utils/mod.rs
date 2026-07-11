//! ABI test helpers.

use std::sync::Arc;

use alloy::json_abi::JsonAbi;

use crate::AbiService;

/// A simple ABI JSON for testing purposes.
pub fn erc20_abi_json() -> &'static str {
    include_str!("../../../../abis/erc20.json")
}

/// Creates a test `AbiService` with the given ABIs registered in memory.
pub async fn create_test_abi_service(abis: &[(&str, &str)]) -> Arc<AbiService> {
    let abi_service = Arc::new(AbiService::new());

    for (name, content) in abis {
        let abi: JsonAbi = serde_json::from_str(content).unwrap();
        abi_service.register_abi((*name).to_string(), Arc::new(abi));
    }

    abi_service
}
