use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::loader::{Loadable, LoaderError};

/// Configuration for an action
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActionConfig {
    /// Name of the action
    pub name: String,
    /// Path to the action file
    pub file: PathBuf,
}

impl Loadable for ActionConfig {
    type Error = LoaderError;

    const KEY: &'static str = "actions";

    fn validate(&self) -> Result<(), Self::Error> {
        if self.name.trim().is_empty() {
            return Err(LoaderError::ValidationError("Action name cannot be empty".into()));
        }
        if self.file.as_os_str().is_empty() {
            return Err(LoaderError::ValidationError("Action file path cannot be empty".into()));
        }

        // Verify file exists
        match self.file.try_exists() {
            Ok(true) => Ok(()),
            Ok(false) =>
                return Err(LoaderError::ValidationError(format!(
                    "Action file does not exist: {}",
                    self.file.display()
                ))),
            Err(e) =>
                return Err(LoaderError::ValidationError(format!(
                    "Failed to access action file {}: {}",
                    self.file.display(),
                    e
                ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_config_validation() {
        let valid_config =
            ActionConfig { name: "Valid Action".into(), file: "actions/action.js".into() };
        assert!(valid_config.validate().is_ok());

        let invalid_config = ActionConfig { name: "".into(), file: "".into() };
        assert!(invalid_config.validate().is_err());
    }

    #[test]
    fn test_action_config_file_existence() {
        let non_existent_file_config =
            ActionConfig { name: "NonExistent".into(), file: "non_existent_file.rs".into() };
        assert!(non_existent_file_config.validate().is_err());

        // Create a temporary file to test existence
        let temp_dir = tempfile::tempdir().unwrap();
        let temp_action_file = temp_dir.path().join("temp_action.js");
        std::fs::write(&temp_action_file, "console.log('Hello, world!');").unwrap();

        let existent_file_config =
            ActionConfig { name: "Existent".into(), file: temp_action_file.into() };
        assert!(existent_file_config.validate().is_ok());
    }
}
