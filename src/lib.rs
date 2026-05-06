#![warn(missing_docs)]
//! Argus core library.
//! Contains the main components and modules for the Argus application.
//! This includes monitoring, alerting, persistence, action dispatching, and
//! more.

pub use argus_api::{action, http_server};
pub use argus_engine::{abi, engine, monitor};

pub mod cmd;
pub mod context;
pub mod loader;
pub mod supervisor;
pub mod test_helpers;
