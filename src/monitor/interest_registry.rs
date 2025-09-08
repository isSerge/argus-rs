use std::{collections::HashSet, sync::Arc};

use alloy::primitives::{Address, B256};

use crate::models::Log;

/// A registry for quickly determining if a log or calldata is of interest to any monitor.
#[derive(Debug, Default)]
pub struct InterestRegistry {
    /// Set of addresses that have log-aware monitors.
    pub log_addresses: Arc<HashSet<Address>>,

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
        self.log_addresses.contains(&log.address()) || self.is_globally_monitored(log)
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
            None => false, // Contract creation - not interesting for calldata-aware monitors
        }
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::{address, b256};

    use super::*;
    use crate::test_helpers::LogBuilder;

    #[test]
    fn test_is_log_interesting() {
        let monitored_address = address!("0000000000000000000000000000000000000001");
        let other_address = address!("0000000000000000000000000000000000000002");
        let monitored_event_sig =
            b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef");
        let other_event_sig =
            b256!("8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b925");

        let registry = InterestRegistry {
            log_addresses: Arc::new(HashSet::from([monitored_address])),
            calldata_addresses: Arc::new(HashSet::new()),
            global_event_signatures: Arc::new(HashSet::from([monitored_event_sig])),
        };

        // Case 1: Log from a monitored address
        let log1 = LogBuilder::new().address(monitored_address).build();
        assert!(registry.is_log_interesting(&log1));

        // Case 2: Log from an unmonitored address, but with a globally monitored
        // event signature
        let log2 = LogBuilder::new().address(other_address).topic(monitored_event_sig).build();
        assert!(registry.is_log_interesting(&log2));

        // Case 3: Log from a monitored address and with a globally monitored event
        // signature
        let log3 = LogBuilder::new().address(monitored_address).topic(monitored_event_sig).build();
        assert!(registry.is_log_interesting(&log3));

        // Case 4: Log from an unmonitored address with an unmonitored event
        // signature
        let log4 = LogBuilder::new().address(other_address).topic(other_event_sig).build();
        assert!(!registry.is_log_interesting(&log4));

        // Case 5: Log from a monitored address with an unmonitored event signature
        let log5 = LogBuilder::new().address(monitored_address).topic(other_event_sig).build();
        assert!(registry.is_log_interesting(&log5));

        // Case 6: Log from an unmonitored address with no topics
        let log6 = LogBuilder::new().address(other_address).build();
        assert!(!registry.is_log_interesting(&log6));

        // Case 7: Log from a monitored address with no topics
        let log7 = LogBuilder::new().address(monitored_address).build();
        assert!(registry.is_log_interesting(&log7));
    }

    #[test]
    fn test_is_calldata_interesting() {
        let monitored_address = address!("0000000000000000000000000000000000000001");
        let other_address = address!("0000000000000000000000000000000000000002");

        let registry = InterestRegistry {
            log_addresses: Arc::new(HashSet::new()),
            calldata_addresses: Arc::new(HashSet::from([monitored_address])),
            global_event_signatures: Arc::new(HashSet::new()),
        };

        // Case 1: Calldata to a monitored address
        assert!(registry.is_calldata_interesting(&Some(monitored_address)));

        // Case 2: Calldata to an unmonitored address
        assert!(!registry.is_calldata_interesting(&Some(other_address)));

        // Case 3: Contract creation (to: None)
        assert!(!registry.is_calldata_interesting(&None));

        // Case 4: Registry with no calldata-aware addresses
        let empty_registry = InterestRegistry::default();
        assert!(!empty_registry.is_calldata_interesting(&Some(monitored_address)));
    }

    #[test]
    fn test_is_log_interesting_edge_cases() {
        let monitored_address = address!("1111111111111111111111111111111111111111");
        let monitored_event_sig =
            b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef");

        // Case 1: Registry with no global signatures
        let registry_no_globals = InterestRegistry {
            log_addresses: Arc::new(HashSet::from([monitored_address])),
            calldata_addresses: Arc::new(HashSet::new()),
            global_event_signatures: Arc::new(HashSet::new()), // Empty set
        };

        let log_from_other_addr = LogBuilder::new()
            .address(address!("2222222222222222222222222222222222222222"))
            .topic(monitored_event_sig) // This topic is NOT in the registry
            .build();

        assert!(!registry_no_globals.is_log_interesting(&log_from_other_addr));

        // Case 2: Log with no topics
        let log_no_topics =
            LogBuilder::new().address(address!("3333333333333333333333333333333333333333")).build(); // No .topic() calls

        let registry_with_globals = InterestRegistry {
            log_addresses: Arc::new(HashSet::new()),
            calldata_addresses: Arc::new(HashSet::new()),
            global_event_signatures: Arc::new(HashSet::from([monitored_event_sig])),
        };

        // Should return false because it can't match a global signature
        assert!(!registry_with_globals.is_log_interesting(&log_no_topics));
    }
}
