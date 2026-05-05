#![warn(missing_docs)]
//! Argus core library.
//! Contains the main components and modules for the Argus application.
//! This includes monitoring, alerting, persistence, action dispatching, and
//! more.

pub mod abi;
pub mod action;
pub mod cmd;
pub mod context;
pub mod engine;
pub mod http_server;
pub mod loader;
pub mod monitor;
pub mod supervisor;
pub mod test_helpers;
