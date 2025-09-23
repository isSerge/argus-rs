use argus_models::config::ActionConfig;

use crate::loader::{Loadable, LoaderError};

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
            Ok(false) => Err(LoaderError::ValidationError(format!(
                "Action file does not exist: {}",
                self.file.display()
            ))),
            Err(e) => Err(LoaderError::ValidationError(format!(
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
        // Invalid config with empty name and file - should fail
        let invalid_config = ActionConfig { name: "".into(), file: "".into() };
        assert!(invalid_config.validate().is_err());

        // Non-existent file - should fail
        let non_existent_file_config =
            ActionConfig { name: "NonExistent".into(), file: "non_existent_file.rs".into() };
        assert!(non_existent_file_config.validate().is_err());

        // Valid existent file - should pass
        // Create a temporary file to test existence
        let temp_dir = tempfile::tempdir().unwrap();
        let temp_action_file = temp_dir.path().join("temp_action.js");
        std::fs::write(&temp_action_file, "console.log('Hello, world!');").unwrap();

        let existent_file_config =
            ActionConfig { name: "Existent".into(), file: temp_action_file.into() };
        assert!(existent_file_config.validate().is_ok());
    }
}
