use std::{fs, path::PathBuf};

use thiserror::Error;

use crate::{
    config::{ConfigLoader, LoaderError},
    models::monitor::MonitorConfig,
};

/// Loads monitor configurations from a file.
pub struct MonitorLoader {
    path: PathBuf,
}

/// Errors that can occur while loading monitor configurations.
#[derive(Debug, Error)]
pub enum MonitorLoaderError {
    /// An error occurred during the loading process.
    #[error("Failed to load monitor configuration: {0}")]
    Loader(#[from] LoaderError),

    /// Error when resolving the ABI path.
    #[error("Failed to resolve ABI path: {0}")]
    AbiPathError(String),
}

impl MonitorLoader {
    /// Creates a new `MonitorLoader` instance.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Loads the monitor configuration from the specified file.
    pub fn load(&self) -> Result<Vec<MonitorConfig>, MonitorLoaderError> {
        let loader = ConfigLoader::new(self.path.clone());
        let mut monitors: Vec<MonitorConfig> = loader.load("monitors")?;

        // Resolve ABI paths to be absolute
        let base_dir = self.path.parent().unwrap_or_else(|| std::path::Path::new(""));

        for monitor in &mut monitors {
            if let Some(abi_path_str) = &monitor.abi {
                let abi_path = base_dir.join(abi_path_str);
                let absolute_abi_path = fs::canonicalize(&abi_path).map_err(|e| {
                    MonitorLoaderError::AbiPathError(format!(
                        "Failed to find ABI file at {}: {}",
                        abi_path.display(),
                        e
                    ))
                })?;
                monitor.abi = Some(absolute_abi_path.to_string_lossy().to_string());
            }
        }

        Ok(monitors)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    /// Helper for creating test directories with configuration files.
    /// Optionally creates an ABI file if `abi_filename` and `abi_content` are
    /// Some.
    fn create_test_dir_with_files(
        yaml_filename: &str,
        yaml_content: &str,
        abi_filename: Option<&str>,
        abi_content: Option<&str>,
    ) -> (TempDir, PathBuf) {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let yaml_path = temp_dir.path().join(yaml_filename);
        fs::write(&yaml_path, yaml_content).expect("Failed to write YAML file");

        if let (Some(name), Some(content)) = (abi_filename, abi_content) {
            let abi_path = temp_dir.path().join(name);
            fs::write(&abi_path, content).expect("Failed to write ABI file");
        }

        (temp_dir, yaml_path)
    }

    fn create_test_yaml_content() -> String {
        r#"
monitors:
  - name: "USDC Transfer Monitor"
    network: "ethereum"
    address: "0xa0b86a33e6441b38d4b5e5bfa1bf7a5eb70c5b1e"
    filter_script: |
      log.name == "Transfer" && 
      bigint(log.params.value) > bigint("1000000000")
    notifiers:
        - "test-notifier"

  - name: "DEX Swap Monitor"
    network: "ethereum"
    address: "0x7a250d5630b4cf539739df2c5dacb4c659f2488d"
    filter_script: "log.name == \"Swap\""

  - name: "Native ETH Transfer Monitor"
    network: "ethereum"
    filter_script: "bigint(tx.value) > bigint(\"1000000000000000000\")"
"#
        .trim()
        .to_string()
    }

    fn create_test_yaml_with_abi_path(abi_path_str: &str) -> (String, String) {
        let abi_content = r#"
[
    {
        "type": "event",
        "name": "Transfer",
        "inputs": [
            {"name": "from", "type": "address", "indexed": true},
            {"name": "to", "type": "address", "indexed": true},
            {"name": "value", "type": "uint256", "indexed": false}
        ]
    }
]
"#
        .trim()
        .to_string();

        let yaml_content = format!(
            r#"
monitors:
  - name: "USDC Transfer Monitor"
    network: "ethereum"
    address: "0xa0b86a33e6441b38d4b5e5bfa1bf7a5eb70c5b1e"
    abi: "{}"
    filter_script: "log.name == 'Transfer'"
  - name: "Native ETH Transfer Monitor"
    network: "ethereum"
    filter_script: "tx.value > 1000"
"#,
            abi_path_str
        );

        (yaml_content, abi_content)
    }

    #[test]
    fn test_load_with_abi_file_resolves_to_absolute_path() {
        let (yaml_content, abi_content) = create_test_yaml_with_abi_path("./usdc.json");
        let (temp_dir, yaml_path) = create_test_dir_with_files(
            "monitors.yaml",
            &yaml_content,
            Some("usdc.json"),
            Some(&abi_content),
        );

        let loader = MonitorLoader::new(yaml_path);
        let result = loader.load();

        assert!(result.is_ok());
        let monitors = result.unwrap();
        assert_eq!(monitors.len(), 2);

        // Check the monitor with the ABI
        let usdc_monitor = &monitors[0];
        assert_eq!(usdc_monitor.name, "USDC Transfer Monitor");
        assert!(usdc_monitor.abi.is_some());

        let expected_abi_path = temp_dir.path().join("usdc.json");
        let expected_abs_path = fs::canonicalize(expected_abi_path).unwrap();

        assert_eq!(
            usdc_monitor.abi.as_ref().unwrap(),
            &expected_abs_path.to_string_lossy().to_string()
        );

        // Check the monitor without the ABI
        let eth_monitor = &monitors[1];
        assert_eq!(eth_monitor.name, "Native ETH Transfer Monitor");
        assert!(eth_monitor.abi.is_none());
    }

    #[test]
    fn test_load_with_nonexistent_abi_file() {
        let (yaml_content, _) = create_test_yaml_with_abi_path("./nonexistent.json");
        let (_temp_dir, yaml_path) =
            create_test_dir_with_files("monitors.yaml", &yaml_content, None, None);

        let loader = MonitorLoader::new(yaml_path);
        let result = loader.load();

        assert!(result.is_err());
        matches!(result.unwrap_err(), MonitorLoaderError::AbiPathError(_));
    }

    #[test]
    fn test_load_valid_yaml_file() {
        let content = create_test_yaml_content();
        let (_temp_dir, file_path) =
            create_test_dir_with_files("monitors.yaml", &content, None, None);

        let loader = MonitorLoader::new(file_path);
        let result = loader.load();

        println!("Result: {:?}", result);

        assert!(result.is_ok());
        let monitors = result.unwrap();
        assert_eq!(monitors.len(), 3);

        // Check first monitor (with address)
        assert_eq!(monitors[0].name, "USDC Transfer Monitor");
        assert_eq!(monitors[0].network, "ethereum");
        assert_eq!(
            monitors[0].address,
            Some("0xa0b86a33e6441b38d4b5e5bfa1bf7a5eb70c5b1e".to_string())
        );
        assert!(monitors[0].filter_script.contains("Transfer"));
        assert_eq!(monitors[0].notifiers, vec!["test-notifier".to_string()]);

        // Check second monitor (with address)
        assert_eq!(monitors[1].name, "DEX Swap Monitor");
        assert_eq!(monitors[1].network, "ethereum");
        assert_eq!(
            monitors[1].address,
            Some("0x7a250d5630b4cf539739df2c5dacb4c659f2488d".to_string())
        );
        assert_eq!(monitors[1].filter_script, "log.name == \"Swap\"");

        // Check third monitor (without address)
        assert_eq!(monitors[2].name, "Native ETH Transfer Monitor");
        assert_eq!(monitors[2].network, "ethereum");
        assert_eq!(monitors[2].address, None);
        assert_eq!(monitors[2].filter_script, "bigint(tx.value) > bigint(\"1000000000000000000\")");
    }

    #[test]
    fn test_load_valid_yml_extension() {
        let content = create_test_yaml_content();
        let (_temp_dir, file_path) =
            create_test_dir_with_files("monitors.yml", &content, None, None);

        let loader = MonitorLoader::new(file_path);
        let result = loader.load();

        assert!(result.is_ok());
        let monitors = result.unwrap();
        assert_eq!(monitors.len(), 3);
    }

    #[test]
    fn test_load_empty_yaml_file() {
        let content = "monitors: []"; // Empty monitors array
        let (_temp_dir, file_path) = create_test_dir_with_files("empty.yaml", content, None, None);

        let loader = MonitorLoader::new(file_path);
        let result = loader.load();

        assert!(result.is_ok());
        let monitors = result.unwrap();
        assert_eq!(monitors.len(), 0);
    }

    #[test]
    fn test_load_nonexistent_file() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let file_path = temp_dir.path().join("nonexistent.yaml");

        let loader = MonitorLoader::new(file_path);
        let result = loader.load();

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, MonitorLoaderError::Loader(LoaderError::IoError(_))));
    }

    #[test]
    fn test_load_unsupported_extension() {
        let content = create_test_yaml_content();
        let (_temp_dir, file_path) =
            create_test_dir_with_files("monitors.json", &content, None, None);

        let loader = MonitorLoader::new(file_path);
        let result = loader.load();

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, MonitorLoaderError::Loader(LoaderError::UnsupportedFormat)));
    }

    #[test]
    fn test_load_no_extension() {
        let content = create_test_yaml_content();
        let (_temp_dir, file_path) = create_test_dir_with_files("monitors", &content, None, None);

        let loader = MonitorLoader::new(file_path);
        let result = loader.load();

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, MonitorLoaderError::Loader(LoaderError::UnsupportedFormat)));
    }

    #[test]
    fn test_load_invalid_yaml_syntax() {
        let invalid_content = r#"
monitors:
  - name: "Invalid Monitor"
    network: "ethereum"
    address: "0x123"
    filter_script: |
      some script
    unclosed_bracket: [
invalid_yaml: {key without value
"#;
        let (_temp_dir, file_path) =
            create_test_dir_with_files("invalid.yaml", invalid_content, None, None);

        let loader = MonitorLoader::new(file_path);
        let result = loader.load();

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, MonitorLoaderError::Loader(LoaderError::ParseError(_))));
    }

    #[test]
    fn test_load_missing_required_fields() {
        let invalid_content = r#"
monitors:
  - name: "Incomplete Monitor"
    network: "ethereum"
    # Missing address and filter_script
"#;
        let (_temp_dir, file_path) =
            create_test_dir_with_files("incomplete.yaml", invalid_content, None, None);

        let loader = MonitorLoader::new(file_path);
        let result = loader.load();

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, MonitorLoaderError::Loader(LoaderError::ParseError(_))));
    }
}
