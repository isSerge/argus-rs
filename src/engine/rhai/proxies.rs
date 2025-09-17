//! Defines proxy objects for script-side access to decoded data.
//!
//! These proxies wrap optional data (e.g., `Option<DecodedLog>`) and expose
//! properties to the Rhai script engine. When the underlying data is `None`,
//! the proxies return safe, default values, preventing script errors and
//! eliminating the need for optional-chaining (`?.`) in user scripts.

use std::sync::Arc;

use alloy::primitives::Address;
use rhai::{Dynamic, Map};

use crate::{
    abi::{DecodedCall, DecodedLog},
    engine::rhai::conversions::build_params_map,
};

/// A proxy for accessing parameters of decoded logs and calls in Rhai scripts.
#[derive(Clone)]
pub struct ParamsProxy(Option<Map>);

impl ParamsProxy {
    /// Indexer to access parameters by name.
    /// Returns `Dynamic::UNIT` if the parameter does not exist or if there are
    /// no parameters.
    fn index_get(&mut self, index: &str) -> Dynamic {
        if let Some(map) = &self.0 {
            map.get(index).cloned().unwrap_or(Dynamic::UNIT)
        } else {
            Dynamic::UNIT
        }
    }
}

/// A proxy for accessing decoded logs in Rhai scripts.
#[derive(Clone)]
pub struct LogProxy(pub Option<Arc<DecodedLog>>);

impl LogProxy {
    /// Gets the name of the log event.
    /// Returns an empty string if there is no decoded log.
    fn get_name(&mut self) -> String {
        self.0.as_ref().map_or_else(|| "".to_string(), |log| log.name.clone())
    }

    /// Gets the address of the contract that emitted the log.
    /// Returns the zero address if there is no decoded log.
    fn get_address(&mut self) -> String {
        self.0
            .as_ref()
            .map_or_else(|| Address::ZERO.to_string(), |log| log.log.address().to_checksum(None))
    }

    /// Gets the log index within the block.
    /// Returns 0 if there is no decoded log or if the log index is unavailable.
    fn get_log_index(&mut self) -> u64 {
        self.0.as_ref().map_or(0, |log| log.log.log_index().unwrap_or(0))
    }

    /// Gets the parameters of the decoded log as a `ParamsProxy`.
    /// Returns an empty `ParamsProxy` if there is no decoded log.
    fn get_params(&mut self) -> ParamsProxy {
        ParamsProxy(self.0.as_ref().map(|log| build_params_map(&log.params)))
    }
}

/// A proxy for accessing decoded calls in Rhai scripts.
#[derive(Clone)]
pub struct CallProxy(pub Option<Arc<DecodedCall>>);

impl CallProxy {
    /// Gets the name of the function call.
    /// Returns an empty string if there is no decoded call.
    fn get_name(&mut self) -> String {
        self.0.as_ref().map_or_else(|| "".to_string(), |call| call.name.clone())
    }

    /// Gets the parameters of the decoded call as a `ParamsProxy`.
    /// Returns an empty `ParamsProxy` if there is no decoded call.
    fn get_params(&mut self) -> ParamsProxy {
        ParamsProxy(self.0.as_ref().map(|call| build_params_map(&call.params)))
    }
}

/// Registers the proxy types and their methods with the given Rhai engine.
pub fn register_proxies(engine: &mut rhai::Engine) {
    // LogProxy
    engine.register_type::<LogProxy>();
    engine.register_get("name", LogProxy::get_name);
    engine.register_get("address", LogProxy::get_address);
    engine.register_get("log_index", LogProxy::get_log_index);
    engine.register_get("params", LogProxy::get_params);

    // CallProxy
    engine.register_type::<CallProxy>();
    engine.register_get("name", CallProxy::get_name);
    engine.register_get("params", CallProxy::get_params);

    // ParamsProxy
    engine.register_type::<ParamsProxy>();
    engine.register_indexer_get(ParamsProxy::index_get);
}
