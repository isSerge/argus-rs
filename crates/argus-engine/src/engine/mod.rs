//! The Argus Engine is responsible for processing blockchain data and providing
//! filtering capabilities.

pub mod alert_manager;
pub mod block_ingestor;
pub mod block_processor;
pub mod filtering;
pub mod outbox_processor;

pub use argus_abi as abi;
pub use argus_monitor as monitor;
pub use argus_rhai as rhai;
