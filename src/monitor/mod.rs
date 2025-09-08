//! Monitor module for managing and validating monitoring configurations.

mod interest_registry;
mod manager;
mod validator;

pub use interest_registry::InterestRegistry;
pub use manager::{GLOBAL_MONITORS_KEY, MonitorManager};
pub use validator::{MonitorValidationError, MonitorValidator};
