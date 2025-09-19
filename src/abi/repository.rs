//! This module provides the `AbiRepository` for loading and managing contract
//! ABIs from a designated directory.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use alloy::json_abi::JsonAbi;
use thiserror::Error;

/// Errors that can occur while loading ABIs into the repository.
#[derive(Debug, Error)]
pub enum AbiRepositoryError {
    /// An I/O error occurred while reading an ABI file.
    #[error("Failed to read ABI file '{path}': {source}")]
    IoError {
        /// The path to the ABI file.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// An error occurred while parsing an ABI JSON.
    #[error("Failed to parse ABI JSON from '{path}': {source}")]
    ParseError {
        /// The path to the ABI file.
        path: PathBuf,
        /// The underlying JSON parsing error.
        #[source]
        source: serde_json::Error,
    },
    /// An error occurred while scanning the ABI directory.
    #[error("Failed to scan ABI directory '{path}': {source}")]
    ScanError {
        /// The path to the ABI directory.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
}

/// A repository for contract ABIs, loading them all once from a directory.
#[derive(Debug, Default, Clone)]
pub struct AbiRepository {
    /// A map from ABI name (filename without extension) to its parsed
    /// `JsonAbi`.
    abis: HashMap<String, Arc<JsonAbi>>,
}

impl AbiRepository {
    /// Creates a new `AbiRepository` by scanning the specified directory for
    /// `*.json` files and loading them.
    pub fn new(abi_dir: &Path) -> Result<Self, AbiRepositoryError> {
        let mut abis = HashMap::new();

        if !abi_dir.exists() {
            tracing::warn!("ABI directory does not exist: {}", abi_dir.display());
            return Ok(Self { abis });
        }

        // Read the ABI directory
        let result = fs::read_dir(abi_dir).map_err(|e| AbiRepositoryError::ScanError {
            path: abi_dir.to_path_buf(),
            source: e,
        })?;

        for entry in result {
            let entry = entry.map_err(|e| AbiRepositoryError::ScanError {
                path: abi_dir.to_path_buf(),
                source: e,
            })?;
            let path = entry.path();

            if path.is_file() && path.extension().is_some_and(|ext| ext == "json") {
                let abi_name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| {
                        tracing::warn!("Could not get file stem for ABI file: {}", path.display());
                        // This unwrap is safe because `path.is_file()` ensures it has a file name.
                        path.file_name().unwrap().to_string_lossy().into_owned()
                    });

                tracing::debug!("Loading ABI: {}", path.display());
                let content = fs::read_to_string(&path)
                    .map_err(|e| AbiRepositoryError::IoError { path: path.clone(), source: e })?;

                let abi: JsonAbi = serde_json::from_str(&content).map_err(|e| {
                    AbiRepositoryError::ParseError { path: path.clone(), source: e }
                })?;

                abis.insert(abi_name, Arc::new(abi));
            }
        }

        tracing::info!("Loaded {} ABIs from {}", abis.len(), abi_dir.display());
        Ok(Self { abis })
    }

    /// Retrieves an `Arc<JsonAbi>` by its name.
    pub fn get_abi(&self, name: &str) -> Option<Arc<JsonAbi>> {
        self.abis.get(name).map(Arc::clone)
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
    use std::{collections::HashSet, io::Write};

    use tempfile::tempdir;

    use super::*;
    use crate::test_helpers::erc20_abi_json;

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

    fn create_test_abi_file(dir: &tempfile::TempDir, filename: &str, content: &str) -> PathBuf {
        let file_path = dir.path().join(filename);
        let mut file = fs::File::create(&file_path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file_path
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

    #[test]
    fn test_abi_repository_new_success() {
        let temp_dir = tempdir().unwrap();
        create_test_abi_file(&temp_dir, "erc20.json", erc20_abi_json());
        create_test_abi_file(&temp_dir, "weth.json", weth_abi_content());
        create_test_abi_file(&temp_dir, "not_an_abi.txt", "some text"); // Should be ignored

        let repo = AbiRepository::new(temp_dir.path()).unwrap();

        assert_eq!(repo.len(), 2);
        assert!(repo.get_abi("erc20").is_some());
        assert!(repo.get_abi("weth").is_some());
        assert!(repo.get_abi("nonexistent").is_none());

        let erc20_abi = repo.get_abi("erc20").unwrap();
        let function_names: HashSet<_> = erc20_abi.functions().map(|f| f.name.clone()).collect();
        for required in REQUIRED_ERC20_FUNCTIONS {
            assert!(
                function_names.contains(*required),
                "Missing required ERC-20 function: {}",
                required
            );
        }
    }

    #[test]
    fn test_abi_repository_new_empty_directory() {
        let temp_dir = tempdir().unwrap();
        let repo = AbiRepository::new(temp_dir.path()).unwrap();
        assert!(repo.is_empty());
    }

    #[test]
    fn test_abi_repository_new_nonexistent_directory() {
        let non_existent_path = PathBuf::from("/tmp/non_existent_abi_dir_12345");
        let repo = AbiRepository::new(&non_existent_path).unwrap();
        assert!(repo.is_empty());
    }

    #[test]
    fn test_abi_repository_new_invalid_json() {
        let temp_dir = tempdir().unwrap();
        create_test_abi_file(&temp_dir, "invalid.json", "{invalid json");

        let result = AbiRepository::new(temp_dir.path());
        assert!(result.is_err());
        match result.unwrap_err() {
            AbiRepositoryError::ParseError { path, .. } => {
                assert_eq!(path, temp_dir.path().join("invalid.json"));
            }
            _ => panic!("Expected ParseError"),
        }
    }

    #[test]
    fn test_abi_repository_get_abi() {
        let temp_dir = tempdir().unwrap();
        create_test_abi_file(&temp_dir, "test_abi.json", erc20_abi_json());
        let repo = AbiRepository::new(temp_dir.path()).unwrap();

        let abi = repo.get_abi("test_abi");
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

        let non_existent_abi = repo.get_abi("another_abi");
        assert!(non_existent_abi.is_none());
    }

    #[test]
    fn test_abi_repository_len_and_is_empty() {
        let temp_dir = tempdir().unwrap();
        let repo_empty = AbiRepository::new(temp_dir.path()).unwrap();
        assert_eq!(repo_empty.len(), 0);
        assert!(repo_empty.is_empty());

        create_test_abi_file(&temp_dir, "one.json", erc20_abi_json());
        let repo_one = AbiRepository::new(temp_dir.path()).unwrap();
        assert_eq!(repo_one.len(), 1);
        assert!(!repo_one.is_empty());

        create_test_abi_file(&temp_dir, "two.json", weth_abi_content());
        let repo_two = AbiRepository::new(temp_dir.path()).unwrap();
        assert_eq!(repo_two.len(), 2);
        assert!(!repo_two.is_empty());
    }
}
