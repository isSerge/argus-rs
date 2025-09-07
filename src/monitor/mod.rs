//! Monitor module for managing and validating monitoring configurations.

mod manager;
mod validator;

pub use validator::{MonitorValidationError, MonitorValidator};
