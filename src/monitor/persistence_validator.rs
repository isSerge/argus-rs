//! Validator for monitor persistence operations.

use std::sync::Arc;

use thiserror::Error;

use super::validator::{MonitorValidationError, MonitorValidator};
use crate::{
    models::monitor::MonitorConfig,
    persistence::{error::PersistenceError, traits::AppRepository},
};

/// An error that occurs during monitor persistence validation.
#[derive(Debug, Error)]
pub enum MonitorPersistenceValidationError {
    /// An error from the monitor's business logic validation.
    #[error(transparent)]
    BusinessLogic(#[from] MonitorValidationError),

    /// An error from the persistence layer.
    #[error(transparent)]
    Persistence(#[from] PersistenceError),

    /// The monitor name already exists.
    #[error("Monitor with name '{0}' already exists.")]
    NameConflict(String),
}

/// A validator for monitor persistence operations.
pub struct MonitorPersistenceValidator<'a> {
    repo: Arc<dyn AppRepository>,
    network_id: String,
    business_logic_validator: &'a MonitorValidator,
}

impl<'a> MonitorPersistenceValidator<'a> {
    /// Creates a new `MonitorPersistenceValidator`.
    pub fn new(
        repo: Arc<dyn AppRepository>,
        network_id: &str,
        business_logic_validator: &'a MonitorValidator,
    ) -> Self {
        Self { repo, network_id: network_id.to_string(), business_logic_validator }
    }

    /// Validates a `MonitorConfig` for creation via HTTP API.
    pub async fn validate_for_create(
        &self,
        monitor: &MonitorConfig,
    ) -> Result<(), MonitorPersistenceValidationError> {
        // 1. Validate network matches
        if monitor.network != self.network_id {
            return Err(MonitorPersistenceValidationError::BusinessLogic(
                MonitorValidationError::InvalidNetwork {
                    monitor_name: monitor.name.clone(),
                    expected_network: self.network_id.clone(),
                    actual_network: monitor.network.clone(),
                },
            ));
        }

        // 2. Check for name uniqueness within the network
        let existing_monitors = self.repo.get_monitors(&self.network_id).await?;
        if existing_monitors.iter().any(|m| m.name == monitor.name) {
            return Err(MonitorPersistenceValidationError::NameConflict(monitor.name.clone()));
        }

        // 3. Perform business logic validation (scripts, ABIs, templates)
        self.business_logic_validator.validate(monitor)?;

        Ok(())
    }

    /// Validates a `MonitorConfig` for update via HTTP API.
    ///
    /// Checks that no other monitor has the same name. The monitor being
    /// updated is allowed to keep its own name.
    ///
    /// Note: Existence check is handled by the repository layer which returns
    /// PersistenceError::NotFound if the monitor doesn't exist during the
    /// actual update.
    pub async fn validate_for_update(
        &self,
        monitor_id: &str,
        monitor: &MonitorConfig,
    ) -> Result<(), MonitorPersistenceValidationError> {
        // 1. Validate network matches
        if monitor.network != self.network_id {
            return Err(MonitorPersistenceValidationError::BusinessLogic(
                MonitorValidationError::InvalidNetwork {
                    monitor_name: monitor.name.clone(),
                    expected_network: self.network_id.clone(),
                    actual_network: monitor.network.clone(),
                },
            ));
        }

        // 2. Check for name uniqueness - if another monitor with this name exists,
        // ensure it's the same monitor we're updating
        let existing_monitors = self.repo.get_monitors(&self.network_id).await?;
        if let Some(existing) = existing_monitors.iter().find(|m| m.name == monitor.name)
            && existing.id.to_string() != monitor_id
        {
            return Err(MonitorPersistenceValidationError::NameConflict(monitor.name.clone()));
        }

        // 3. Perform business logic validation (scripts, ABIs, templates)
        self.business_logic_validator.validate(monitor)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::Address;

    use super::*;
    use crate::{
        persistence::traits::MockAppRepository,
        test_helpers::{MonitorBuilder, create_monitor_validator},
    };

    #[tokio::test]
    async fn monitor_persistence_validator_validates_for_create() {
        const NETWORK_ID: &str = "testnet";
        let mut repo = MockAppRepository::new();

        repo.expect_get_monitors()
            .withf(move |network| network == NETWORK_ID)
            .returning(|_| Ok(vec![]));

        let business_logic_validator = create_monitor_validator(&[], None).await;

        let validator =
            MonitorPersistenceValidator::new(Arc::new(repo), NETWORK_ID, &business_logic_validator);

        let monitor = MonitorConfig {
            name: "Test Monitor".into(),
            network: NETWORK_ID.into(),
            address: Some(Address::default().to_checksum(None)),
            abi_name: Some("erc20".to_string()),
            filter_script: "true".to_string(),
            actions: vec![],
        };

        let result = validator.validate_for_create(&monitor).await;

        assert!(
            result.is_ok(),
            "Expected validation to pass, got error: {:?}",
            result.unwrap_err()
        );
    }

    #[tokio::test]
    async fn monitor_persistence_validator_validates_for_create_network_mismatch() {
        const NETWORK_ID: &str = "not-testnet"; // default monitor validator uses "testnet"
        let mut repo = MockAppRepository::new();

        repo.expect_get_monitors()
            .withf(move |network| network == NETWORK_ID)
            .returning(|_| Ok(vec![]));

        let business_logic_validator = create_monitor_validator(&[], None).await;

        let validator =
            MonitorPersistenceValidator::new(Arc::new(repo), NETWORK_ID, &business_logic_validator);

        let monitor = MonitorConfig {
            name: "Test Monitor".into(),
            network: NETWORK_ID.into(),
            address: Some(Address::default().to_checksum(None)),
            abi_name: Some("erc20".to_string()),
            filter_script: "true".to_string(),
            actions: vec![],
        };

        let result = validator.validate_for_create(&monitor).await;

        assert!(matches!(
            result,
            Err(MonitorPersistenceValidationError::BusinessLogic(
                MonitorValidationError::InvalidNetwork { .. }
            ))
        ));
    }

    #[tokio::test]
    async fn monitor_persistence_validator_validates_for_create_name_conflict() {
        const NETWORK_ID: &str = "testnet"; // default monitor validator uses "testnet"
        const MONITOR_NAME: &str = "Test Monitor";
        let mut repo = MockAppRepository::new();

        // Create an existing monitor with the same name and network
        let existing_monitor = MonitorBuilder::new().name(MONITOR_NAME).network(NETWORK_ID).build();

        repo.expect_get_monitors()
            .withf(move |network| network == NETWORK_ID)
            .returning(move |_| Ok(vec![existing_monitor.clone()]));

        let business_logic_validator = create_monitor_validator(&[], None).await;

        let validator =
            MonitorPersistenceValidator::new(Arc::new(repo), NETWORK_ID, &business_logic_validator);

        let monitor = MonitorConfig {
            name: MONITOR_NAME.into(),
            network: NETWORK_ID.into(),
            address: Some(Address::default().to_checksum(None)),
            abi_name: Some("erc20".to_string()),
            filter_script: "true".to_string(),
            actions: vec![],
        };

        let result = validator.validate_for_create(&monitor).await;

        assert!(matches!(
            result,
            Err(MonitorPersistenceValidationError::NameConflict(name)) if name == MONITOR_NAME
        ));
    }

    #[tokio::test]
    async fn monitor_persistence_validator_validate_for_update_success_same_name() {
        const NETWORK_ID: &str = "testnet";
        let mut repo = MockAppRepository::new();

        // existing monitor with id 1 and same name
        let existing = MonitorBuilder::new().id(1).name("existing").network(NETWORK_ID).build();
        repo.expect_get_monitors()
            .withf(move |network| network == NETWORK_ID)
            .returning(move |_| Ok(vec![existing.clone()]));

        let business_logic_validator = create_monitor_validator(&[], None).await;
        let validator =
            MonitorPersistenceValidator::new(Arc::new(repo), NETWORK_ID, &business_logic_validator);

        let monitor_id = "1".to_string();
        let monitor = MonitorConfig {
            name: "existing".into(),
            network: NETWORK_ID.into(),
            address: None,
            abi_name: None,
            filter_script: "true".to_string(),
            actions: vec![],
        };

        let result = validator.validate_for_update(&monitor_id, &monitor).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn monitor_persistence_validator_validate_for_update_success_new_name() {
        const NETWORK_ID: &str = "testnet";
        let mut repo = MockAppRepository::new();

        // existing monitor with a different name
        let existing = MonitorBuilder::new().id(1).name("old-name").network(NETWORK_ID).build();
        repo.expect_get_monitors()
            .withf(move |network| network == NETWORK_ID)
            .returning(move |_| Ok(vec![existing.clone()]));

        let business_logic_validator = create_monitor_validator(&[], None).await;
        let validator =
            MonitorPersistenceValidator::new(Arc::new(repo), NETWORK_ID, &business_logic_validator);

        let monitor_id = "1".to_string();
        let monitor = MonitorConfig {
            name: "new-name".into(),
            network: NETWORK_ID.into(),
            address: None,
            abi_name: None,
            filter_script: "true".to_string(),
            actions: vec![],
        };

        let result = validator.validate_for_update(&monitor_id, &monitor).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn monitor_persistence_validator_validate_for_update_name_conflict() {
        const NETWORK_ID: &str = "testnet";
        let mut repo = MockAppRepository::new();

        // two monitors: id 1 and id 2
        let m1 = MonitorBuilder::new().id(1).name("monitor-1").network(NETWORK_ID).build();
        let m2 = MonitorBuilder::new().id(2).name("monitor-2").network(NETWORK_ID).build();
        repo.expect_get_monitors()
            .withf(move |network| network == NETWORK_ID)
            .returning(move |_| Ok(vec![m1.clone(), m2.clone()]));

        let business_logic_validator = create_monitor_validator(&[], None).await;
        let validator =
            MonitorPersistenceValidator::new(Arc::new(repo), NETWORK_ID, &business_logic_validator);

        let monitor1_id = "1".to_string();
        let updated = MonitorConfig {
            name: "monitor-2".into(),
            network: NETWORK_ID.into(),
            address: None,
            abi_name: None,
            filter_script: "true".to_string(),
            actions: vec![],
        };

        let result = validator.validate_for_update(&monitor1_id, &updated).await;
        assert!(matches!(result, Err(MonitorPersistenceValidationError::NameConflict(_))));
    }

    #[tokio::test]
    async fn monitor_persistence_validator_validate_for_update_network_mismatch() {
        const NETWORK_ID: &str = "testnet";
        let mut repo = MockAppRepository::new();

        // existing monitor present
        let existing = MonitorBuilder::new().id(1).name("monitor").network(NETWORK_ID).build();
        repo.expect_get_monitors()
            .withf(move |network| network == NETWORK_ID)
            .returning(move |_| Ok(vec![existing.clone()]));

        let business_logic_validator = create_monitor_validator(&[], None).await;
        let validator =
            MonitorPersistenceValidator::new(Arc::new(repo), NETWORK_ID, &business_logic_validator);

        let monitor_id = "1".to_string();
        let updated = MonitorConfig {
            name: "monitor".into(),
            network: "mainnet".into(),
            address: None,
            abi_name: None,
            filter_script: "true".to_string(),
            actions: vec![],
        };

        let result = validator.validate_for_update(&monitor_id, &updated).await;
        assert!(matches!(
            result,
            Err(MonitorPersistenceValidationError::BusinessLogic(
                MonitorValidationError::InvalidNetwork { .. }
            ))
        ));
    }

    #[tokio::test]
    async fn monitor_persistence_validator_validate_for_update_nonexistent_monitor() {
        const NETWORK_ID: &str = "testnet";
        let mut repo = MockAppRepository::new();

        // no monitors returned
        repo.expect_get_monitors()
            .withf(move |network| network == NETWORK_ID)
            .returning(move |_| Ok(vec![]));

        let business_logic_validator = create_monitor_validator(&[], None).await;
        let validator =
            MonitorPersistenceValidator::new(Arc::new(repo), NETWORK_ID, &business_logic_validator);

        let monitor = MonitorConfig {
            name: "monitor".into(),
            network: NETWORK_ID.into(),
            address: None,
            abi_name: None,
            filter_script: "true".to_string(),
            actions: vec![],
        };

        // Validator should not check for existence
        let result = validator.validate_for_update("999", &monitor).await;
        assert!(result.is_ok());
    }
}
