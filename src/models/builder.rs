//! A builder for creating `Monitor` instances in tests.

use chrono::{DateTime, Utc};

use crate::models::monitor::Monitor;

/// A builder for creating `Monitor` instances in tests.
pub struct MonitorBuilder {
    id: Option<i64>,
    name: Option<String>,
    network: Option<String>,
    address: Option<String>,
    abi: Option<String>,
    filter_script: Option<String>,
    notifiers: Option<Vec<String>>,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
    decode_calldata: Option<bool>,
}

impl Default for MonitorBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl MonitorBuilder {
    /// Creates a new `MonitorBuilder` instance.
    pub fn new() -> Self {
        MonitorBuilder {
            id: None,
            name: None,
            network: None,
            address: None,
            abi: None,
            filter_script: None,
            notifiers: None,
            created_at: None,
            updated_at: None,
            decode_calldata: None,
        }
    }

    /// Sets the address for the monitor.
    pub fn address(mut self, address: &str) -> Self {
        self.address = Some(address.to_string());
        self
    }

    /// Sets the ABI for the monitor.
    pub fn abi(mut self, abi: &str) -> Self {
        self.abi = Some(abi.to_string());
        self
    }

    /// Sets the notifiers for the monitor.
    pub fn notifiers(mut self, notifiers: Vec<String>) -> Self {
        self.notifiers = Some(notifiers);
        self
    }

    /// Sets the creation timestamp for the monitor.
    pub fn created_at(mut self, created_at: DateTime<Utc>) -> Self {
        self.created_at = Some(created_at);
        self
    }

    /// Sets the update timestamp for the monitor.
    pub fn updated_at(mut self, updated_at: DateTime<Utc>) -> Self {
        self.updated_at = Some(updated_at);
        self
    }

    /// Sets the ID for the monitor.
    pub fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    /// Sets the name for the monitor.
    pub fn name(mut self, name: &str) -> Self {
        self.name = Some(name.to_string());
        self
    }

    /// Sets the network for the monitor.
    pub fn network(mut self, network: &str) -> Self {
        self.network = Some(network.to_string());
        self
    }

    /// Sets the filter script for the monitor.
    pub fn filter_script(mut self, script: &str) -> Self {
        self.filter_script = Some(script.to_string());
        self
    }

    /// Builds the `Monitor` instance.
    pub fn build(self) -> Monitor {
        Monitor {
            id: self.id.unwrap_or_default(),
            name: self.name.unwrap_or("test monitor".to_string()),
            network: self.network.unwrap_or("test network".to_string()),
            address: self.address,
            abi: self.abi,
            filter_script: self.filter_script.unwrap_or("true".to_string()),
            notifiers: self.notifiers.unwrap_or_default(),
            created_at: self.created_at.unwrap_or_default(),
            updated_at: self.updated_at.unwrap_or_default(),
            decode_calldata: self.decode_calldata.unwrap_or(false),
        }
    }
}
