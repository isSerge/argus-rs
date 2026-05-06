#![warn(missing_docs)]
//! Argus core library.
//! Contains the main components and modules for the Argus application.
//! This includes monitoring, alerting, persistence, action dispatching, and
//! more.

pub mod cmd;
pub mod context;
pub mod loader;
pub mod supervisor;
pub mod test_helpers;
