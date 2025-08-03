//! This module contains the data models for the Argus application.

pub mod block_data;
pub mod monitor;
pub mod transaction;
pub mod trigger;
pub mod monitor_match;
pub mod correlated_data;

pub use block_data::BlockData;
pub use correlated_data::CorrelatedBlockItem;
