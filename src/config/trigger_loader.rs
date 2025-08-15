//! Loads and validates trigger configurations from a YAML file.

use std::path::PathBuf;

use thiserror::Error;

use super::loader::{ConfigLoader, LoaderError};
use crate::models::trigger::{TriggerConfig, TriggerTypeConfigError};

/// Loads trigger configurations from a file.
pub struct TriggerLoader {
    path: PathBuf,
}

/// Errors that can occur while loading trigger configurations.
#[derive(Debug, Error)]
pub enum TriggerLoaderError {
    /// An error occurred during the loading process.
    #[error("Failed to load trigger configuration: {0}")]
    Loader(#[from] LoaderError),

    /// The trigger configuration is invalid.
    #[error("Invalid trigger configuration: {0}")]
    ValidationError(#[from] TriggerTypeConfigError),
}

impl TriggerLoader {
    /// Creates a new `TriggerLoader` instance.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Loads and validates the trigger configurations from the specified file.
    pub fn load(&self) -> Result<Vec<TriggerConfig>, TriggerLoaderError> {
        let loader = ConfigLoader::new(self.path.clone());
        let triggers: Vec<TriggerConfig> = loader.load("triggers")?;

        for trigger_config in &triggers {
            trigger_config.config.validate()?;
        }

        Ok(triggers)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Write};

    use tempfile::TempDir;

    use super::*;

    fn create_test_file(dir: &TempDir, filename: &str, content: &str) -> PathBuf {
        let path = dir.path().join(filename);
        let mut file = File::create(&path).unwrap();
        writeln!(file, "{}", content).unwrap();
        path
    }

    #[test]
    fn test_load_valid_triggers_success() {
        let dir = TempDir::new().unwrap();
        let content = r#"
triggers:
  - name: "test_webhook"
    webhook:
      url: "http://example.com/webhook"
      message:
        title: "Test Title"
        body: "Test Body"
"#;
        let path = create_test_file(&dir, "triggers.yaml", content);
        let loader = TriggerLoader::new(path);
        let result = loader.load();
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn test_load_invalid_webhook_url_fails() {
        let dir = TempDir::new().unwrap();
        let content = r#"
triggers:
  - name: "invalid_webhook"
    webhook:
      url: "not a valid url"
      message:
        title: "Test Title"
        body: "Test Body"
"#;
        let path = create_test_file(&dir, "triggers.yaml", content);
        let loader = TriggerLoader::new(path);
        let result = loader.load();
        assert!(result.is_err());
        matches!(result.unwrap_err(), TriggerLoaderError::ValidationError(_));
    }

    #[test]
    fn test_load_invalid_slack_url_fails() {
        let dir = TempDir::new().unwrap();
        let content = r#"
triggers:
  - name: "invalid_slack"
    slack:
      slack_url: "http://wrongdomain.com/hook"
      message:
        title: "Test Title"
        body: "Test Body"
"#;
        let path = create_test_file(&dir, "triggers.yaml", content);
        let loader = TriggerLoader::new(path);
        let result = loader.load();
        assert!(result.is_err());
        matches!(result.unwrap_err(), TriggerLoaderError::ValidationError(_));
    }
}
