//! Test helpers for ABI-related functionality.

use std::sync::Arc;

use crate::{
    abi::{AbiRepository, AbiService},
    persistence::{sqlite::SqliteStateRepository, traits::AppRepository},
};

/// A simple ABI JSON for testing purposes.
pub fn erc20_abi_json() -> &'static str {
    // get abi from abis/erc20.json
    include_str!("../../abis/erc20.json")
}

/// Creates a test `AbiService` with the given ABIs written to the database.
pub async fn create_test_abi_service(
    abis: &[(&str, &str)],
) -> (Arc<AbiService>, Arc<AbiRepository>) {
    let repo = SqliteStateRepository::new("sqlite::memory:")
        .await
        .expect("Failed to connect to in-memory db");
    repo.run_migrations().await.expect("Failed to run migrations");

    for (name, content) in abis {
        repo.create_abi(name, content).await.unwrap();
    }

    let abi_repo = Arc::new(AbiRepository::new(Arc::new(repo)).await.unwrap());
    let abi_service = Arc::new(AbiService::new(abi_repo.clone()));
    (abi_service, abi_repo)
}
