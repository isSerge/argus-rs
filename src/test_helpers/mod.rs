//! Test utilities for the argus crate
//!
//! These utilities are only available when the `test-helpers` feature is
//! enabled.
//!
//! # Usage
//!
//! In unit tests:
//! ```rust
//! #[cfg(test)]
//! mod tests {
//!     use crate::test_helpers::*;
//! }
//! ```
//!
//! In integration tests, enable the feature:
//! ```bash
//! cargo test --features test-helpers
//! ```

mod abi;
mod block;
mod http_client;
mod js_client;
mod log;
mod match_manager;
mod monitor_manager;
mod monitor_match;
mod receipt;
mod transaction;

pub use abi::{create_test_abi_service, erc20_abi_json};
pub use block::BlockBuilder;
pub use http_client::create_test_http_client;
pub use js_client::get_shared_js_client;
pub use log::LogBuilder;
pub use match_manager::{create_test_match_manager, create_test_match_manager_with_repo};
pub use monitor_manager::create_test_monitor_manager;
pub use monitor_match::create_monitor_match;
pub use receipt::ReceiptBuilder;
pub use transaction::TransactionBuilder;

pub use crate::models::builder::MonitorBuilder;
