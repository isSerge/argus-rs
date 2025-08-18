//! Loads and validates notifier configurations from a YAML file.

use std::path::PathBuf;

use thiserror::Error;

use super::loader::{ConfigLoader, LoaderError};
use crate::models::notifier::{NotifierConfig, NotifierTypeConfigError};

/// Loads notifier configurations from a file.
pub struct NotifierLoader {
    path: PathBuf,
}

/// Errors that can occur while loading notifier configurations.
#[derive(Debug, Error)]
pub enum NotifierLoaderError {
    /// An error occurred during the loading process.
    #[error("Failed to load notifier configuration: {0}")]
    Loader(#[from] LoaderError),

    /// The notifier configuration is invalid.
    #[error("Invalid notifier configuration: {0}")]
    ValidationError(#[from] NotifierTypeConfigError),
}

impl NotifierLoader {
    /// Creates a new `NotifierLoader` instance.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Loads and validates the notifier configurations from the specified file.
    pub fn load(&self) -> Result<Vec<NotifierConfig>, NotifierLoaderError> {
        let loader = ConfigLoader::new(self.path.clone());
        let notifiers: Vec<NotifierConfig> = loader.load("notifiers")?;

        for notifier_config in &notifiers {
            notifier_config.config.validate()?;
        }

        Ok(notifiers)
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
    fn test_load_valid_notifiers_success() {
        let dir = TempDir::new().unwrap();
        let content = r#"
notifiers:
  - name: "test_webhook"
    webhook:
      url: "http://example.com/webhook"
      message:
        title: "Test Title"
        body: "Test Body"
"#;
        let path = create_test_file(&dir, "notifiers.yaml", content);
        let loader = NotifierLoader::new(path);
        let result = loader.load();
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn test_load_invalid_webhook_url_fails() {
        let dir = TempDir::new().unwrap();
        let content = r#"
notifiers:
  - name: "invalid_webhook"
    webhook:
      url: "not a valid url"
      message:
        title: "Test Title"
        body: "Test Body"
"#;
        let path = create_test_file(&dir, "notifiers.yaml", content);
        let loader = NotifierLoader::new(path);
        let result = loader.load();
        assert!(result.is_err());
        matches!(result.unwrap_err(), NotifierLoaderError::ValidationError(_));
    }

    #[test]
    fn test_load_invalid_slack_url_fails() {
        let dir = TempDir::new().unwrap();
        let content = r#"
notifiers:
  - name: "invalid_slack"
    slack:
      slack_url: "http://wrongdomain.com/hook"
      message:
        title: "Test Title"
        body: "Test Body"
"#;
        let path = create_test_file(&dir, "notifiers.yaml", content);
        let loader = NotifierLoader::new(path);
        let result = loader.load();
        assert!(result.is_err());
        matches!(result.unwrap_err(), NotifierLoaderError::ValidationError(_));
    }
}
