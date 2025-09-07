//! Monitor module for managing and validating monitoring configurations.

mod manager;
mod validator;

pub use manager::{GLOBAL_MONITORS_KEY, MonitorManager};
pub use validator::{MonitorValidationError, MonitorValidator};
