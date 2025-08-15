//! This module provides the `AbiLoader` for loading contract ABIs from files.

use std::{fs, path::PathBuf};

use alloy::json_abi::JsonAbi;
use thiserror::Error;

/// Errors that can occur while loading an ABI.
#[derive(Debug, Error)]
pub enum AbiLoaderError {
    /// An I/O error occurred while reading the ABI file.
    #[error("Failed to read ABI file '{path}': {source}")]
    IoError {
        /// The path to the ABI file.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// An error occurred while parsing the ABI JSON.
    #[error("Failed to parse ABI JSON from '{path}': {source}")]
    ParseError {
        /// The path to the ABI file.
        path: PathBuf,
        /// The underlying JSON parsing error.
        #[source]
        source: serde_json::Error,
    },
}

/// A loader for contract ABIs from files.
pub struct AbiLoader {
    path: PathBuf,
}

impl AbiLoader {
    /// Creates a new `AbiLoader` instance.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Loads and parses the ABI from the specified file.
    pub fn load(&self) -> Result<JsonAbi, AbiLoaderError> {
        let content = fs::read_to_string(&self.path)
            .map_err(|e| AbiLoaderError::IoError { path: self.path.clone(), source: e })?;

        let abi = serde_json::from_str(&content)
            .map_err(|e| AbiLoaderError::ParseError { path: self.path.clone(), source: e })?;

        Ok(abi)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::tempdir;

    use super::*;

    fn create_test_abi_file(dir: &tempfile::TempDir, filename: &str, content: &str) -> PathBuf {
        let file_path = dir.path().join(filename);
        let mut file = fs::File::create(&file_path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file_path
    }

    #[test]
    fn test_abi_loader_success() {
        let temp_dir = tempdir().unwrap();
        let abi_content = r#"
[
    {
        "type": "function",
        "name": "balanceOf",
        "inputs": [
            {"name": "account", "type": "address"}
        ],
        "outputs": [
            {"name": "", "type": "uint256"}
        ]
    }
]
"#;
        let abi_path = create_test_abi_file(&temp_dir, "erc20.json", abi_content);

        let loader = AbiLoader::new(abi_path);
        let result = loader.load();

        assert!(result.is_ok());
        let abi = result.unwrap();
        assert_eq!(abi.functions().count(), 1);
        assert_eq!(abi.functions().next().unwrap().name, "balanceOf");
    }

    #[test]
    fn test_abi_loader_nonexistent_file() {
        let temp_dir = tempdir().unwrap();
        let abi_path = temp_dir.path().join("nonexistent.json");

        let loader = AbiLoader::new(abi_path.clone());
        let result = loader.load();

        assert!(result.is_err());
        match result.unwrap_err() {
            AbiLoaderError::IoError { path, .. } => {
                assert_eq!(path, abi_path);
            }
            _ => panic!("Expected IoError"),
        }
    }

    #[test]
    fn test_abi_loader_invalid_json() {
        let temp_dir = tempdir().unwrap();
        let abi_path = create_test_abi_file(&temp_dir, "invalid.json", "{invalid json");

        let loader = AbiLoader::new(abi_path.clone());
        let result = loader.load();

        assert!(result.is_err());
        match result.unwrap_err() {
            AbiLoaderError::ParseError { path, .. } => {
                assert_eq!(path, abi_path);
            }
            _ => panic!("Expected ParseError"),
        }
    }
}
