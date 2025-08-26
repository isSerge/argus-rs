//! Generic configuration loading utilities.

use std::{fs, path::PathBuf};

use config::{Config, File, FileFormat};
use serde::de::DeserializeOwned;
use thiserror::Error;

/// Errors that can occur during configuration loading.
#[derive(Debug, Error)]
pub enum LoaderError {
    /// Error when reading the configuration file.
    #[error("Failed to read configuration file: {0}")]
    IoError(#[from] std::io::Error),

    /// Error when parsing the configuration file.
    #[error("Failed to parse configuration: {0}")]
    ParseError(#[from] config::ConfigError),

    /// Error when the configuration format is unsupported.
    #[error("Unsupported configuration format")]
    UnsupportedFormat,
}

/// A generic loader for YAML files.
pub struct ConfigLoader {
    path: PathBuf,
}

impl ConfigLoader {
    /// Creates a new `ConfigLoader`.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Loads a vector of items from the YAML file.
    /// The generic type `T` must be deserializable.
    /// The `key` parameter specifies the top-level key in the YAML file
    /// that holds the list of items (e.g., "monitors", "triggers").
    pub fn load<T: DeserializeOwned>(&self, key: &str) -> Result<Vec<T>, LoaderError> {
        if !self.is_yaml_file() {
            return Err(LoaderError::UnsupportedFormat);
        }

        let config_str = fs::read_to_string(&self.path)?;

        let config =
            Config::builder().add_source(File::from_str(&config_str, FileFormat::Yaml)).build()?;

        let items = config.get(key)?;

        Ok(items)
    }

    /// Checks if the file has a YAML extension.
    fn is_yaml_file(&self) -> bool {
        matches!(self.path.extension().and_then(|ext| ext.to_str()), Some("yaml") | Some("yml"))
    }
}

/// A trait for types that can be loaded from a configuration file.
pub trait Loadable: Sized + DeserializeOwned {
    /// The top-level key in the YAML file (e.g., "monitors").
    const KEY: &'static str;

    /// The specific error type for this loadable item.
    type Error: From<LoaderError>;

    /// A method for post-deserialization logic, such as validation.
    ///
    /// This method has a default no-op implementation, making it optional
    /// for types that don't require specific processing.
    fn validate(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Loads a vector of `Loadable` items from a configuration file.
pub fn load_config<T: Loadable>(path: PathBuf) -> Result<Vec<T>, T::Error> {
    let loader = ConfigLoader::new(path.clone());
    let mut items: Vec<T> = loader.load(T::KEY)?;

    for item in &mut items {
        item.validate()?;
    }

    Ok(items)
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Write};

    use serde::Deserialize;
    use tempfile::TempDir;

    use super::*;

    #[derive(Debug, Deserialize, PartialEq)]
    struct TestItem {
        name: String,
        value: i32,
    }

    fn create_test_file(dir: &TempDir, filename: &str, content: &str) -> PathBuf {
        let path = dir.path().join(filename);
        let mut file = File::create(&path).unwrap();
        writeln!(file, "{}", content).unwrap();
        path
    }

    #[test]
    fn test_load_success() {
        let dir = TempDir::new().unwrap();
        let content = r#"
items:
  - name: "A"
    value: 1
  - name: "B"
    value: 2
"#;
        let path = create_test_file(&dir, "test.yaml", content);
        let loader = ConfigLoader::new(path);
        let result: Result<Vec<TestItem>, _> = loader.load("items");

        assert!(result.is_ok());
        let items = result.unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0], TestItem { name: "A".into(), value: 1 });
        assert_eq!(items[1], TestItem { name: "B".into(), value: 2 });
    }

    #[test]
    fn test_load_nonexistent_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nonexistent.yaml");
        let loader = ConfigLoader::new(path);
        let result: Result<Vec<TestItem>, _> = loader.load("items");

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), LoaderError::IoError(_)));
    }

    #[test]
    fn test_load_invalid_yaml_syntax() {
        let dir = TempDir::new().unwrap();
        let content = "items: [ { name: 'A', value: 1 }, { name: 'B', value: 2"; // Missing closing bracket
        let path = create_test_file(&dir, "invalid.yaml", content);
        let loader = ConfigLoader::new(path);
        let result: Result<Vec<TestItem>, _> = loader.load("items");

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), LoaderError::ParseError(_)));
    }

    #[test]
    fn test_load_unsupported_format() {
        let dir = TempDir::new().unwrap();
        let path = create_test_file(&dir, "test.txt", "items: []");
        let loader = ConfigLoader::new(path);
        let result: Result<Vec<TestItem>, _> = loader.load("items");

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), LoaderError::UnsupportedFormat));
    }

    #[test]
    fn test_load_missing_top_level_key() {
        let dir = TempDir::new().unwrap();
        let content = r#"
wrong_key:
  - name: "A"
    value: 1
"#;
        let path = create_test_file(&dir, "test.yaml", content);
        let loader = ConfigLoader::new(path);
        let result: Result<Vec<TestItem>, _> = loader.load("items");

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), LoaderError::ParseError(_)));
    }
}
