use config::{Config, File};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use thiserror::Error;

use crate::models::monitor::Monitor;

/// Container for monitor configurations loaded from file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorConfigFile {
    pub monitors: Vec<Monitor>,
}

/// Loads monitor configurations from a file.
pub struct MonitorLoader {
    path: PathBuf,
}

/// Errors that can occur while loading monitor configurations.
#[derive(Debug, Error)]
pub enum MonitorLoaderError {
    /// Error when reading the monitor configuration file.
    #[error("Failed to load monitor configuration: {0}")]
    IoError(std::io::Error),

    /// Error when parsing the monitor configuration file.
    #[error("Failed to parse monitor configuration: {0}")]
    ParseError(String),

    /// Error when the monitor configuration format is unsupported.
    #[error("Unsupported monitor configuration format")]
    UnsupportedFormat,
}

impl MonitorLoader {
    /// Creates a new `MonitorLoader` instance.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Loads the monitor configuration from the specified file.
    pub fn load(&self) -> Result<Vec<Monitor>, MonitorLoaderError> {
        // Validate YAML extension
        if !self.is_yaml_file() {
            return Err(MonitorLoaderError::UnsupportedFormat);
        }

        let config_str = fs::read_to_string(&self.path).map_err(MonitorLoaderError::IoError)?;
        let config: MonitorConfigFile = Config::builder()
            .add_source(File::from_str(&config_str, config::FileFormat::Yaml))
            .build()
            .map_err(|e| MonitorLoaderError::ParseError(e.to_string()))?
            .try_deserialize()
            .map_err(|e| MonitorLoaderError::ParseError(e.to_string()))?;
        Ok(config.monitors)
    }

    /// Checks if the file has a YAML extension.
    fn is_yaml_file(&self) -> bool {
        matches!(
            self.path.extension().and_then(|ext| ext.to_str()),
            Some("yaml") | Some("yml")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_yaml_content() -> String {
        r#"
monitors:
  - name: "USDC Transfer Monitor"
    network: "ethereum"
    address: "0xa0b86a33e6441b38d4b5e5bfa1bf7a5eb70c5b1e"
    filter_script: |
      log.name == "Transfer" && 
      bigint(log.params.value) > bigint("1000000000")

  - name: "DEX Swap Monitor"
    network: "ethereum"
    address: "0x7a250d5630b4cf539739df2c5dacb4c659f2488d"
    filter_script: "log.name == \"Swap\""
"#
        .trim()
        .to_string()
    }

    fn create_test_dir_with_file(filename: &str, content: &str) -> (TempDir, PathBuf) {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let file_path = temp_dir.path().join(filename);
        fs::write(&file_path, content).expect("Failed to write test file");
        (temp_dir, file_path)
    }

    #[test]
    fn test_load_valid_yaml_file() {
        let content = create_test_yaml_content();
        let (_temp_dir, file_path) = create_test_dir_with_file("monitors.yaml", &content);

        let loader = MonitorLoader::new(file_path);
        let result = loader.load();

        assert!(result.is_ok());
        let monitors = result.unwrap();
        assert_eq!(monitors.len(), 2);

        // Check first monitor
        assert_eq!(monitors[0].name, "USDC Transfer Monitor");
        assert_eq!(monitors[0].network, "ethereum");
        assert_eq!(
            monitors[0].address,
            "0xa0b86a33e6441b38d4b5e5bfa1bf7a5eb70c5b1e"
        );
        assert!(monitors[0].filter_script.contains("Transfer"));
        assert_eq!(monitors[0].id, 0); // Default value from serde

        // Check second monitor
        assert_eq!(monitors[1].name, "DEX Swap Monitor");
        assert_eq!(monitors[1].network, "ethereum");
        assert_eq!(
            monitors[1].address,
            "0x7a250d5630b4cf539739df2c5dacb4c659f2488d"
        );
        assert_eq!(monitors[1].filter_script, "log.name == \"Swap\"");
    }

    #[test]
    fn test_load_valid_yml_extension() {
        let content = create_test_yaml_content();
        let (_temp_dir, file_path) = create_test_dir_with_file("monitors.yml", &content);

        let loader = MonitorLoader::new(file_path);
        let result = loader.load();

        assert!(result.is_ok());
        let monitors = result.unwrap();
        assert_eq!(monitors.len(), 2);
    }

    #[test]
    fn test_load_empty_yaml_file() {
        let content = "monitors: []"; // Empty monitors array
        let (_temp_dir, file_path) = create_test_dir_with_file("empty.yaml", content);

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
        matches!(result.unwrap_err(), MonitorLoaderError::IoError(_));
    }

    #[test]
    fn test_load_unsupported_extension() {
        let content = create_test_yaml_content();
        let (_temp_dir, file_path) = create_test_dir_with_file("monitors.json", &content);

        let loader = MonitorLoader::new(file_path);
        let result = loader.load();

        assert!(result.is_err());
        matches!(result.unwrap_err(), MonitorLoaderError::UnsupportedFormat);
    }

    #[test]
    fn test_load_no_extension() {
        let content = create_test_yaml_content();
        let (_temp_dir, file_path) = create_test_dir_with_file("monitors", &content);

        let loader = MonitorLoader::new(file_path);
        let result = loader.load();

        assert!(result.is_err());
        matches!(result.unwrap_err(), MonitorLoaderError::UnsupportedFormat);
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
        let (_temp_dir, file_path) = create_test_dir_with_file("invalid.yaml", invalid_content);

        let loader = MonitorLoader::new(file_path);
        let result = loader.load();

        assert!(result.is_err());
        matches!(result.unwrap_err(), MonitorLoaderError::ParseError(_));
    }

    #[test]
    fn test_load_missing_required_fields() {
        let invalid_content = r#"
monitors:
  - name: "Incomplete Monitor"
    network: "ethereum"
    # Missing address and filter_script
"#;
        let (_temp_dir, file_path) = create_test_dir_with_file("incomplete.yaml", invalid_content);

        let loader = MonitorLoader::new(file_path);
        let result = loader.load();

        assert!(result.is_err());
        matches!(result.unwrap_err(), MonitorLoaderError::ParseError(_));
    }

    #[test]
    fn test_is_yaml_file() {
        let loader_yaml = MonitorLoader::new(PathBuf::from("test.yaml"));
        assert!(loader_yaml.is_yaml_file());

        let loader_yml = MonitorLoader::new(PathBuf::from("test.yml"));
        assert!(loader_yml.is_yaml_file());

        let loader_json = MonitorLoader::new(PathBuf::from("test.json"));
        assert!(!loader_json.is_yaml_file());

        let loader_txt = MonitorLoader::new(PathBuf::from("test.txt"));
        assert!(!loader_txt.is_yaml_file());

        let loader_no_ext = MonitorLoader::new(PathBuf::from("test"));
        assert!(!loader_no_ext.is_yaml_file());
    }
}
