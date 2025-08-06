//! This module contains the data models for the Argus application.

pub mod block_data;
pub mod correlated_data;
pub mod log;
pub mod monitor;
pub mod monitor_match;
pub mod transaction;
pub mod trigger;
pub mod decoded_block;

pub use block_data::BlockData;
pub use correlated_data::CorrelatedBlockItem;
pub use log::Log;
pub use decoded_block::DecodedBlockData;
