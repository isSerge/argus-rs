//! Monitor module for managing and validating monitoring configurations.

mod interest_registry;
mod manager;
mod persistence_validator;
mod validator;

pub use interest_registry::InterestRegistryBuilder;
pub use manager::{ClassifiedMonitor, MonitorAssetState, MonitorCapabilities, MonitorManager};
pub use persistence_validator::{MonitorPersistenceValidationError, MonitorPersistenceValidator};
pub use validator::{MonitorValidationError, MonitorValidator};
