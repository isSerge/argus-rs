//! This module provides the `AbiRepository` for loading and managing contract
//! ABIs from the database.

use std::{collections::HashMap, sync::Arc};

use alloy::json_abi::JsonAbi;
use thiserror::Error;

use crate::persistence::{error::PersistenceError, traits::AppRepository};

/// Errors that can occur while loading ABIs into the repository.
#[derive(Debug, Error)]
pub enum AbiRepositoryError {
    /// An error occurred while parsing an ABI JSON.
    #[error("Failed to parse ABI JSON for '{name}': {source}")]
    ParseError {
        /// The name of the ABI.
        name: String,
        /// The underlying JSON parsing error.
        #[source]
        source: serde_json::Error,
    },
    /// An error occurred while fetching ABIs from the database.
    #[error("Failed to fetch ABIs from database: {0}")]
    DatabaseError(#[from] PersistenceError),
}

/// A repository for contract ABIs, loading them all once from the database.
#[derive(Debug, Default, Clone)]
pub struct AbiRepository {
    /// A map from ABI name to its parsed `JsonAbi`.
    abis: HashMap<String, Arc<JsonAbi>>,
}

impl AbiRepository {
    /// Creates a new `AbiRepository` by loading all ABIs from the database.
    pub async fn new(repo: Arc<dyn AppRepository>) -> Result<Self, AbiRepositoryError> {
        let mut abis = HashMap::new();

        let abi_records = repo.get_all_abis().await?;

        for (name, abi_json) in abi_records {
            let abi: JsonAbi = serde_json::from_str(&abi_json)
                .map_err(|e| AbiRepositoryError::ParseError { name: name.clone(), source: e })?;
            abis.insert(name, Arc::new(abi));
        }

        tracing::info!("Loaded {} ABIs from database", abis.len());
        Ok(Self { abis })
    }

    /// Retrieves an `Arc<JsonAbi>` by its name.
    pub fn get_abi(&self, name: &str) -> Option<Arc<JsonAbi>> {
        self.abis.get(name).map(Arc::clone)
    }

    /// Returns a list of all ABI names in the repository.
    pub fn list_abi_names(&self) -> Vec<String> {
        self.abis.keys().cloned().collect()
    }

    /// Returns the number of ABIs in the repository.
    pub fn len(&self) -> usize {
        self.abis.len()
    }

    /// Returns true if the repository contains no ABIs.
    pub fn is_empty(&self) -> bool {
        self.abis.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::{
        persistence::{sqlite::SqliteStateRepository, traits::AppRepository},
        test_helpers::erc20_abi_json,
    };

    const REQUIRED_ERC20_FUNCTIONS: &[&str] = &[
        "transfer",
        "approve",
        "balanceOf",
        "transferFrom",
        "totalSupply",
        "allowance",
        "decimals",
        "symbol",
        "name",
    ];

    async fn setup_test_db() -> Arc<SqliteStateRepository> {
        let repo = SqliteStateRepository::new("sqlite::memory:")
            .await
            .expect("Failed to connect to in-memory db");
        repo.run_migrations().await.expect("Failed to run migrations");
        Arc::new(repo)
    }

    fn weth_abi_content() -> &'static str {
        r#"[
            {
                "type": "function",
                "name": "deposit",
                "inputs": [],
                "outputs": []
            }
        ]"#
    }

    #[tokio::test]
    async fn test_abi_repository_new_success() {
        let repo = setup_test_db().await;
        repo.create_abi("erc20", erc20_abi_json()).await.unwrap();
        repo.create_abi("weth", weth_abi_content()).await.unwrap();

        let abi_repo = AbiRepository::new(repo).await.unwrap();

        assert_eq!(abi_repo.len(), 2);
        assert!(abi_repo.get_abi("erc20").is_some());
        assert!(abi_repo.get_abi("weth").is_some());
        assert!(abi_repo.get_abi("nonexistent").is_none());

        let erc20_abi = abi_repo.get_abi("erc20").unwrap();
        let function_names: HashSet<_> = erc20_abi.functions().map(|f| f.name.clone()).collect();
        for required in REQUIRED_ERC20_FUNCTIONS {
            assert!(
                function_names.contains(*required),
                "Missing required ERC-20 function: {}",
                required
            );
        }
    }

    #[tokio::test]
    async fn test_abi_repository_new_empty_database() {
        let repo = setup_test_db().await;
        let abi_repo = AbiRepository::new(repo).await.unwrap();
        assert!(abi_repo.is_empty());
    }

    #[tokio::test]
    async fn test_abi_repository_new_invalid_json() {
        let repo = setup_test_db().await;
        repo.create_abi("invalid", "{invalid json").await.unwrap();

        let result = AbiRepository::new(repo).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            AbiRepositoryError::ParseError { name, .. } => {
                assert_eq!(name, "invalid");
            }
            _ => panic!("Expected ParseError"),
        }
    }

    #[tokio::test]
    async fn test_abi_repository_get_abi() {
        let repo = setup_test_db().await;
        repo.create_abi("test_abi", erc20_abi_json()).await.unwrap();
        let abi_repo = AbiRepository::new(repo).await.unwrap();

        let abi = abi_repo.get_abi("test_abi");
        assert!(abi.is_some());
        let abi = abi.unwrap();
        let function_names: HashSet<_> = abi.functions().map(|f| f.name.clone()).collect();
        for required in REQUIRED_ERC20_FUNCTIONS {
            assert!(
                function_names.contains(*required),
                "Missing required ERC-20 function: {}",
                required
            );
        }

        let non_existent_abi = abi_repo.get_abi("another_abi");
        assert!(non_existent_abi.is_none());
    }

    #[tokio::test]
    async fn test_abi_repository_len_and_is_empty() {
        let repo_empty_db = setup_test_db().await;
        let repo_empty = AbiRepository::new(repo_empty_db).await.unwrap();
        assert_eq!(repo_empty.len(), 0);
        assert!(repo_empty.is_empty());

        let repo_one_db = setup_test_db().await;
        repo_one_db.create_abi("one", erc20_abi_json()).await.unwrap();
        let repo_one = AbiRepository::new(repo_one_db).await.unwrap();
        assert_eq!(repo_one.len(), 1);
        assert!(!repo_one.is_empty());

        let repo_two_db = setup_test_db().await;
        repo_two_db.create_abi("one", erc20_abi_json()).await.unwrap();
        repo_two_db.create_abi("two", weth_abi_content()).await.unwrap();
        let repo_two = AbiRepository::new(repo_two_db).await.unwrap();
        assert_eq!(repo_two.len(), 2);
        assert!(!repo_two.is_empty());
    }

    #[tokio::test]
    async fn test_list_abi_names() {
        let repo = setup_test_db().await;
        repo.create_abi("erc20", erc20_abi_json()).await.unwrap();
        repo.create_abi("weth", weth_abi_content()).await.unwrap();

        let abi_repo = AbiRepository::new(repo).await.unwrap();
        let mut names = abi_repo.list_abi_names();
        names.sort(); // Sort for deterministic comparison

        assert_eq!(names, vec!["erc20", "weth"]);
    }
}
