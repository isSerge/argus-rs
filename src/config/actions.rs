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
            return Err(LoaderError::ValidationError(
                "Action name cannot be empty".into(),
            ));
        }
        if self.file.as_os_str().is_empty() {
            return Err(LoaderError::ValidationError(
                "Action file path cannot be empty".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_config_validation() {
        let valid_config = ActionConfig {
            name: "Valid Action".into(),
            file: "actions/valid_action.rs".into(),
        };
        assert!(valid_config.validate().is_ok());

        let invalid_config = ActionConfig {
            name: "".into(),
            file: "".into(),
        };
        assert!(invalid_config.validate().is_err());
    }
}
