//! Validator for `ActionConfig`.

use std::sync::Arc;

use thiserror::Error;

use crate::{
    models::action::{ActionConfig, ActionTypeConfigError},
    persistence::{error::PersistenceError, traits::AppRepository},
};

/// An error that occurs during action validation.
#[derive(Debug, Error)]
pub enum ActionValidationError {
    /// An error from the action's intrinsic validation.
    #[error(transparent)]
    Configuration(#[from] ActionTypeConfigError),

    /// An error from the persistence layer.
    #[error(transparent)]
    Persistence(#[from] PersistenceError),

    /// The action name already exists.
    #[error("Action with name '{0}' already exists.")]
    NameConflict(String),
}

/// A validator for `ActionConfig`.
#[derive(Clone)]
pub struct ActionValidator {
    repo: Arc<dyn AppRepository>,
    network_id: String,
}

impl ActionValidator {
    /// Creates a new `ActionValidator`.
    pub fn new(repo: Arc<dyn AppRepository>, network_id: &str) -> Self {
        Self { repo, network_id: network_id.to_string() }
    }

    /// Validates an `ActionConfig` for creation.
    pub async fn validate_for_create(
        &self,
        action: &ActionConfig,
    ) -> Result<(), ActionValidationError> {
        // 1. Perform intrinsic validation.
        action.config.validate()?;

        // 2. Check for name uniqueness.
        if self.repo.get_action_by_name(&self.network_id, &action.name).await?.is_some() {
            return Err(ActionValidationError::NameConflict(action.name.clone()));
        }

        Ok(())
    }

    /// Validates an `ActionConfig` for an update.
    pub async fn validate_for_update(
        &self,
        action: &ActionConfig,
    ) -> Result<(), ActionValidationError> {
        // 1. Perform intrinsic validation.
        action.config.validate()?;

        // 2. Ensure the action exists.
        let action_id = action.id.ok_or(PersistenceError::InvalidInput(
            "Action ID is required for update".to_string(),
        ))?;
        if self.repo.get_action_by_id(&self.network_id, action_id).await?.is_none() {
            return Err(ActionValidationError::Persistence(PersistenceError::NotFound));
        }

        // 3. Check for name uniqueness, ignoring the current action's ID.
        if let Some(existing_action) =
            self.repo.get_action_by_name(&self.network_id, &action.name).await?
        {
            if existing_action.id != action.id {
                return Err(ActionValidationError::NameConflict(action.name.clone()));
            }
        }

        Ok(())
    }
}
