//! Argus core library.
//!
//! Provides the shared models, configuration, persistence traits, and
//! provider traits used across the Argus workspace.

pub mod action_dispatcher;
pub mod config;
pub mod loader;
pub mod metrics;
pub mod models;
pub mod monitor;
pub mod persistence;
pub mod providers;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;
