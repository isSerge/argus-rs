#![warn(missing_docs)]
//! Argus monitoring application.
//!
//! This is the main crate for the Argus application, orchestrating
//! blockchain monitoring, alert evaluation, and notification dispatch.

pub mod cmd;
pub mod context;
pub mod loader;
pub mod supervisor;
