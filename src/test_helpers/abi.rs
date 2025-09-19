//! Test helpers for ABI-related functionality.

use std::{fs, sync::Arc};

use tempfile::TempDir;

use crate::abi::{AbiRepository, AbiService};

/// A simple ABI JSON for testing purposes.
pub fn erc20_abi_json() -> &'static str {
    // get abi from abis/erc20.json
    include_str!("../../abis/erc20.json")
}

/// Creates a test `AbiService` with the given ABIs written to temporary files.
pub fn create_test_abi_service(
    temp_dir: &TempDir,
    abis: &[(&str, &str)],
) -> (Arc<AbiService>, Arc<AbiRepository>) {
    for (name, content) in abis {
        let file_path = temp_dir.path().join(format!("{}.json", name));
        fs::write(file_path, *content).unwrap();
    }

    let abi_repo = Arc::new(AbiRepository::new(temp_dir.path()).unwrap());
    let abi_service = Arc::new(AbiService::new(abi_repo.clone()));
    (abi_service, abi_repo)
}
