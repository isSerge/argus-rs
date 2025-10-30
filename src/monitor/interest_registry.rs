use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use alloy::{
    json_abi::JsonAbi,
    primitives::{Address, B256},
};

use crate::{
    abi::AbiService,
    models::Log,
    monitor::{ClassifiedMonitor, MonitorCapabilities},
};

/// A registry for quickly determining if a log or calldata is of interest to
/// any monitor.
#[derive(Debug, Default)]
pub struct InterestRegistry {
    /// Map of log interests by address.
    /// Some(HashSet<B256>) = Precise Mode: Only logs with these event
    /// signatures are of interest. None = Broad Mode: All logs from this
    /// address are of interest.
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
            None => false, // Contract creation - not interesting for calldata-aware monitors
        }
    }
}

/// A builder for constructing an `InterestRegistry` from classified monitors.
#[derive(Debug, Default)]
pub struct InterestRegistryBuilder {
    log_interests: HashMap<Address, Option<HashSet<B256>>>,
    calldata_addresses: HashSet<Address>,
    global_event_signatures: HashSet<B256>,
}

impl InterestRegistryBuilder {
    /// Adds calldata interest if the monitor has CALL capability and a valid
    /// address.
    fn add_calldata_interest(&mut self, cm: &ClassifiedMonitor) {
        if cm.caps.contains(MonitorCapabilities::CALL)
            && let Some(Ok(addr)) = cm.monitor.address.as_ref().map(|a| a.parse::<Address>())
        {
            self.calldata_addresses.insert(addr);
        }
    }

    /// Adds log interest based on the monitor's address and ABI
    /// if it has LOG capability.
    fn add_log_interest(&mut self, cm: &ClassifiedMonitor, abi_service: &Arc<AbiService>) {
        if !cm.caps.contains(MonitorCapabilities::LOG) {
            return;
        }

        let monitor = &cm.monitor;
        let is_global = monitor.address.as_deref().is_some_and(|a| a.eq_ignore_ascii_case("all"));

        if is_global {
            self.add_global_event_signature(cm, abi_service);
        } else if let Some(Ok(address)) = monitor.address.as_ref().map(|a| a.parse::<Address>()) {
            // --- Handle Address-Specific Monitors ---
            if matches!(self.log_interests.get(&address), Some(&None)) {
                return; // Already in broad mode, can't be more specific.
            }

            if let Some(abi_name) = &monitor.abi_name {
                if let Some(contract) = abi_service.get_abi(address) {
                    // Ensure the correct ABI is linked.
                    if let Some(repo_abi) = abi_service.get_abi_by_name(abi_name)
                        && !Arc::ptr_eq(&contract.abi, &repo_abi)
                    {
                        // Mismatch: monitor specifies an ABI different from the one linked.
                        // For safety, fall back to broad mode.
                        self.log_interests.insert(address, None);
                        return;
                    }

                    let signatures = Self::get_signatures_for_monitor(cm, contract.abi.clone());
                    if let Some(interest_set) =
                        self.log_interests.entry(address).or_insert_with(|| Some(HashSet::new()))
                    {
                        interest_set.extend(signatures);
                    }
                } else {
                    // Safety fallback: ABI not linked for this address, enter broad mode.
                    self.log_interests.insert(address, None);
                }
            } else {
                // Safety fallback: Monitor has an address but no ABI, enter broad mode.
                self.log_interests.insert(address, None);
            }
        }
    }

    /// Adds global event signatures from a global monitor.
    /// A global monitor is one that specifies "all" as its address.
    fn add_global_event_signature(
        &mut self,
        cm: &ClassifiedMonitor,
        abi_service: &Arc<AbiService>,
    ) {
        if let Some(abi_name) = &cm.monitor.abi_name
            && let Some(abi) = abi_service.get_abi_by_name(abi_name)
        {
            self.global_event_signatures.extend(Self::get_signatures_for_monitor(cm, abi));
        }
    }

    /// Adds a classified monitor to the registry builder.
    /// This processes both calldata and log interests.
    pub fn add(&mut self, cm: &ClassifiedMonitor, abi_service: &Arc<AbiService>) {
        self.add_calldata_interest(cm);
        self.add_log_interest(cm, abi_service);
    }

    /// Builds the `InterestRegistry` from the accumulated interests.
    pub fn build(self) -> InterestRegistry {
        InterestRegistry {
            log_interests: Arc::new(self.log_interests),
            calldata_addresses: Arc::new(self.calldata_addresses),
            global_event_signatures: Arc::new(self.global_event_signatures),
        }
    }

    /// Extracts event signatures from the monitor's ABI based on accessed event
    /// names.
    fn get_signatures_for_monitor(cm: &ClassifiedMonitor, abi: Arc<JsonAbi>) -> HashSet<B256> {
        let event_names = &cm.analysis.accessed_log_event_names;

        if !event_names.is_empty() {
            // SMART PATH: Add only the specific event signatures.
            event_names
                .iter()
                .filter_map(|name| abi.event(name))
                .flatten()
                .map(|event| event.selector())
                .collect()
        } else {
            // SAFE PATH: Add all event signatures from the ABI.
            abi.events.values().flatten().map(|event| event.selector()).collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::{address, b256};

    use super::*;
    use crate::{
        engine::rhai::ScriptAnalysis,
        models::monitor::Monitor,
        test_helpers::{LogBuilder, create_test_abi_service, erc20_abi_json},
    };

    const ADDRESS1: Address = address!("0000000000000000000000000000000000000001");
    const ADDRESS2: Address = address!("0000000000000000000000000000000000000002");
    const EVENT_SIG1: B256 =
        b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"); // Transfer(address,address,uint256)
    const EVENT_SIG2: B256 =
        b256!("8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b925"); // Approval(address,address,uint256)

    #[test]
    fn test_is_log_interesting_broad_mode() {
        let registry = InterestRegistry {
            log_interests: Arc::new(HashMap::from([(
                ADDRESS1, None, // Broad mode for this address
            )])),
            calldata_addresses: Arc::new(HashSet::new()),
            global_event_signatures: Arc::new(HashSet::from([EVENT_SIG1])),
        };

        // Case 1: Log from a monitored address
        let log1 = LogBuilder::new().address(ADDRESS1).build();
        assert!(registry.is_log_interesting(&log1));

        // Case 2: Log from an unmonitored address, but with a globally monitored
        // event signature
        let log2 = LogBuilder::new().address(ADDRESS2).topic(EVENT_SIG1).build();
        assert!(registry.is_log_interesting(&log2));

        // Case 3: Log from a monitored address and with a globally monitored event
        // signature
        let log3 = LogBuilder::new().address(ADDRESS1).topic(EVENT_SIG1).build();
        assert!(registry.is_log_interesting(&log3));

        // Case 4: Log from an unmonitored address with an unmonitored event
        // signature
        let log4 = LogBuilder::new().address(ADDRESS2).topic(EVENT_SIG2).build();
        assert!(!registry.is_log_interesting(&log4));

        // Case 5: Log from a monitored address with an unmonitored event signature
        let log5 = LogBuilder::new().address(ADDRESS1).topic(EVENT_SIG2).build();
        assert!(registry.is_log_interesting(&log5));

        // Case 6: Log from an unmonitored address with no topics
        let log6 = LogBuilder::new().address(ADDRESS2).build();
        assert!(!registry.is_log_interesting(&log6));

        // Case 7: Log from a monitored address with no topics
        let log7 = LogBuilder::new().address(ADDRESS1).build();
        assert!(registry.is_log_interesting(&log7));
    }

    #[test]
    fn test_is_log_interesting_precise_mode() {
        let registry = InterestRegistry {
            log_interests: Arc::new(HashMap::from([(
                ADDRESS1,
                Some(HashSet::from([EVENT_SIG1])), // Precise mode for this address
            )])),
            calldata_addresses: Arc::new(HashSet::new()),
            global_event_signatures: Arc::new(HashSet::from([EVENT_SIG1])),
        };

        // Case 1: Log from a monitored address with a monitored event signature
        let log1 = LogBuilder::new().address(ADDRESS1).topic(EVENT_SIG1).build();
        assert!(registry.is_log_interesting(&log1));

        // Case 2: Log from a monitored address with an unmonitored event signature
        let log2 = LogBuilder::new().address(ADDRESS1).topic(EVENT_SIG2).build();
        assert!(!registry.is_log_interesting(&log2));

        // Case 3: Log from an unmonitored address with a globally monitored event
        // signature
        let log3 = LogBuilder::new().address(ADDRESS2).topic(EVENT_SIG1).build();
        assert!(registry.is_log_interesting(&log3));

        // Case 4: Log from an unmonitored address with an unmonitored event
        // signature
        let log4 = LogBuilder::new().address(ADDRESS2).topic(EVENT_SIG2).build();
        assert!(!registry.is_log_interesting(&log4));

        // Case 5: Log from a monitored address with no topics
        let log5 = LogBuilder::new().address(ADDRESS1).build();
        assert!(!registry.is_log_interesting(&log5));
    }

    #[test]
    fn test_is_calldata_interesting() {
        let registry = InterestRegistry {
            log_interests: Arc::new(HashMap::new()),
            calldata_addresses: Arc::new(HashSet::from([ADDRESS1])),
            global_event_signatures: Arc::new(HashSet::new()),
        };

        // Case 1: Calldata to a monitored address
        assert!(registry.is_calldata_interesting(&Some(ADDRESS1)));

        // Case 2: Calldata to an unmonitored address
        assert!(!registry.is_calldata_interesting(&Some(ADDRESS2)));

        // Case 3: Contract creation (to: None)
        assert!(!registry.is_calldata_interesting(&None));

        // Case 4: Registry with no calldata-aware addresses
        let empty_registry = InterestRegistry::default();
        assert!(!empty_registry.is_calldata_interesting(&Some(ADDRESS1)));
    }

    #[test]
    fn test_is_log_interesting_edge_cases() {
        // Case 1: Registry with no global signatures
        let registry_no_globals = InterestRegistry {
            log_interests: Arc::new(HashMap::from([(
                ADDRESS1, None, // Broad mode for this address
            )])),
            calldata_addresses: Arc::new(HashSet::new()),
            global_event_signatures: Arc::new(HashSet::new()), // Empty set
        };

        let log_from_other_addr = LogBuilder::new()
            .address(ADDRESS2)
            .topic(EVENT_SIG2) // This topic is NOT in the registry
            .build();

        assert!(!registry_no_globals.is_log_interesting(&log_from_other_addr));

        // Case 2: Log with no topics
        let log_no_topics =
            LogBuilder::new().address(address!("3333333333333333333333333333333333333333")).build(); // No .topic() calls

        let registry_with_globals = InterestRegistry {
            log_interests: Arc::new(HashMap::new()),
            calldata_addresses: Arc::new(HashSet::new()),
            global_event_signatures: Arc::new(HashSet::from([EVENT_SIG1])),
        };

        // Should return false because it can't match a global signature
        assert!(!registry_with_globals.is_log_interesting(&log_no_topics));
    }

    #[tokio::test]
    async fn test_interest_registry_builder() {
        let (abi_service, _) = create_test_abi_service(&[("ERC20", erc20_abi_json())]).await;

        let mut builder = InterestRegistryBuilder::default();

        let classified_monitor = ClassifiedMonitor {
            monitor: Arc::new(Monitor {
                address: Some(ADDRESS1.to_checksum(None)),
                abi_name: Some("ERC20".into()),
                filter_script: "decoded_call.name == \"transfer\" || log.params.value < ether(10)"
                    .into(), // requires CALL and LOG caps
                ..Default::default()
            }),
            caps: MonitorCapabilities::LOG | MonitorCapabilities::CALL,
            analysis: ScriptAnalysis {
                accessed_log_event_names: HashSet::from([
                    "Transfer".to_string(),
                    "Approval".to_string(),
                ]),
                ..Default::default()
            },
        };

        // Test adding log interest
        builder.add_log_interest(&classified_monitor, &abi_service);

        // Test adding calldata interest
        builder.add_calldata_interest(&classified_monitor);

        // Test building the registry
        let registry = builder.build();

        // Validate the constructed registry
        assert!(
            registry
                .is_log_interesting(&LogBuilder::new().address(ADDRESS1).topic(EVENT_SIG1).build())
        );
        assert!(
            registry
                .is_log_interesting(&LogBuilder::new().address(ADDRESS1).topic(EVENT_SIG2).build())
        );
        assert!(registry.is_calldata_interesting(&Some(ADDRESS1)));
    }
}
