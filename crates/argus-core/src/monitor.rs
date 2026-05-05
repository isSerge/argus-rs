//! A fast-lookup registry for determining if a log or calldata is of interest.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use alloy::primitives::{Address, B256};
use arc_swap::ArcSwap;

use crate::models::Log;

/// A registry for quickly determining if a log or calldata is of interest to
/// any monitor.
#[derive(Debug, Default, Clone)]
pub struct InterestRegistry {
    /// Map of log interests by address.
    /// `Some(HashSet<B256>)` = Precise Mode: Only logs with these event
    /// signatures are of interest.
    /// `None` = Broad Mode: All logs from this address are of interest.
    pub log_interests: Arc<HashMap<Address, Option<HashSet<B256>>>>,

    /// Set of addresses that have calldata-aware monitors.
    pub calldata_addresses: Arc<HashSet<Address>>,

    /// Set of event signatures that global monitors are interested in.
    pub global_event_signatures: Arc<HashSet<B256>>,
}

impl InterestRegistry {
    /// Checks if the given log is of interest based on monitored addresses or
    /// global event signatures.
    #[inline]
    pub fn is_log_interesting(&self, log: &Log) -> bool {
        // 1. Check for specific address interests.
        if let Some(interest_mode) = self.log_interests.get(&log.address()) {
            match interest_mode {
                // Precise Mode: Check if the log's signature is in our set.
                Some(specific_signatures) => {
                    return log
                        .topics()
                        .first()
                        .is_some_and(|topic0| specific_signatures.contains(topic0));
                }
                // Broad Mode: A generic monitor exists for this address, so all its logs are
                // interesting.
                None => return true,
            }
        }

        // 2. If no specific address match, fall back to checking global signatures.
        self.is_globally_monitored(log)
    }

    /// Checks if the log matches any global event signatures.
    #[inline]
    fn is_globally_monitored(&self, log: &Log) -> bool {
        !self.global_event_signatures.is_empty()
            && log
                .topics()
                .first()
                .is_some_and(|topic0| self.global_event_signatures.contains(topic0))
    }

    /// Checks if the given `to_address` is of interest for calldata-aware
    /// monitors.
    #[inline]
    pub fn is_calldata_interesting(&self, to_address: &Option<Address>) -> bool {
        match to_address {
            Some(addr) => self.calldata_addresses.contains(addr),
            None => false,
        }
    }
}

/// Provides live access to the current [`InterestRegistry`].
///
/// Implementors expose a single `interest_registry()` method so that
/// [`EvmRpcSource`] can read the registry without needing to know whether it
/// comes from a standalone `ArcSwap<InterestRegistry>` (tests / isolated use)
/// or from the authoritative `ArcSwap<MonitorAssetState>` (production).  Both
/// cases then share a single atomic snapshot, eliminating the two-`ArcSwap`
/// race that previously existed in `MonitorManager::update()`.
pub trait RegistryProvider: Send + Sync {
    /// Returns a cheap `Arc` handle to the current `InterestRegistry`.
    fn interest_registry(&self) -> Arc<InterestRegistry>;
}

/// Blanket implementation for the standalone case used in tests and anywhere
/// an `ArcSwap<InterestRegistry>` is the source of truth.
impl RegistryProvider for ArcSwap<InterestRegistry> {
    fn interest_registry(&self) -> Arc<InterestRegistry> {
        self.load_full()
    }
}
