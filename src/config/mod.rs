//! Configuration module for Argus.

mod app_config;
mod helpers;
mod http_retry;
mod loader;
mod rhai;
mod rpc_retry;
mod trigger_loader;

pub use app_config::AppConfig;
pub use helpers::{
    deserialize_duration_from_ms, deserialize_duration_from_seconds, deserialize_urls,
    serialize_duration_to_ms, serialize_duration_to_seconds,
};
pub use http_retry::{HttpRetryConfig, JitterSetting};
pub use rhai::RhaiConfig;
pub use rpc_retry::RpcRetryConfig;
pub use trigger_loader::{TriggerLoader, TriggerLoaderError};
pub use loader::{ConfigLoader, LoaderError};
