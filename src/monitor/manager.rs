use std::{collections::HashSet, sync::Arc};

use alloy::primitives::{Address, B256};
use arc_swap::{ArcSwap, Guard};
use dashmap::DashMap;

use crate::{
    abi::AbiService,
    engine::rhai::RhaiCompiler,
    models::{Log, monitor::Monitor},
};

/// The key used to store global log-aware monitors in the `log_aware_monitors`
/// map.
pub const GLOBAL_MONITORS_KEY: &str = "*";

/// A container for monitors that have been organized for efficient execution.
#[derive(Debug, Default)]
pub struct OrganizedMonitors {
    /// Monitors that only access transaction data.
    pub transaction_only_monitors: Vec<Monitor>,
    /// Log-aware monitors, keyed by checksummed contract address or the global
    /// key.
    pub log_aware_monitors: DashMap<String, Vec<Monitor>>,
}

#[derive(Debug, Default)]
pub struct MonitorAssetState {
    /// The full, organized monitor structure for the FilteringEngine.
    pub organized_monitors: OrganizedMonitors,

    /// An optimized, fast-lookup registry for the BlockProcessor.
    pub interest_registry: InterestRegistry,

    /// The flag indicating if any monitor in this snapshot requires receipts.
    pub requires_receipts: bool,
}

/// A registry for quickly determining if a log is of interest to any monitor.
#[derive(Debug, Default)]
pub struct InterestRegistry {
    /// Set of addresses that have log-aware monitors.
    addresses: Arc<HashSet<Address>>,

    /// Set of event signatures that global monitors are interested in.
    global_event_signatures: Arc<HashSet<B256>>,
}

impl InterestRegistry {
    /// Checks if the given log is of interest based on monitored addresses or
    /// global event signatures.
    #[inline]
    pub fn is_log_interesting(&self, log: &Log) -> bool {
        self.addresses.contains(&log.address()) || self.is_globally_monitored(log)
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
}

/// Manages monitors, including organizing them for efficient execution and
/// providing atomic updates to the monitor state.
#[derive(Debug)]
pub struct MonitorManager {
    compiler: Arc<RhaiCompiler>,
    abi_service: Arc<AbiService>,
    state: ArcSwap<MonitorAssetState>,
}

impl MonitorManager {
    /// Creates a new `MonitorManager` with the given initial monitors, Rhai
    /// compiler, and ABI service.
    pub fn new(
        initial_monitors: Vec<Monitor>,
        compiler: Arc<RhaiCompiler>,
        abi_service: Arc<AbiService>,
    ) -> Self {
        let initial_state = Self::organize_assets(&initial_monitors, &compiler, &abi_service);
        Self { compiler, abi_service, state: ArcSwap::new(Arc::new(initial_state)) }
    }

    /// Updates the monitor state with a new set of monitors.
    /// This method atomically swaps the entire monitor state.
    pub fn update(&self, monitors: Vec<Monitor>) {
        let state = Self::organize_assets(&monitors, &self.compiler, &self.abi_service);
        self.state.store(Arc::new(state));
    }

    /// Loads the current snapshot of the monitor state.
    pub fn load(&self) -> Guard<Arc<MonitorAssetState>> {
        self.state.load()
    }

    /// Organizes monitors into the `MonitorAssetState`, categorizing them and
    /// building the interest registry.
    fn organize_assets(
        monitors: &Vec<Monitor>,
        compiler: &Arc<RhaiCompiler>,
        abi_service: &Arc<AbiService>,
    ) -> MonitorAssetState {
        let (organized_monitors, requires_receipts) = Self::organize_monitors(monitors, compiler);
        let interest_registry = Self::build_interest_registry(&organized_monitors, abi_service);
        MonitorAssetState { organized_monitors, requires_receipts, interest_registry }
    }

    /// Organizes monitors into transaction-only and log-aware categories.
    /// Also determines if any monitor requires transaction receipt data.
    fn organize_monitors(
        monitors: &Vec<Monitor>,
        compiler: &Arc<RhaiCompiler>,
    ) -> (OrganizedMonitors, bool) {
        let mut organized_monitors = OrganizedMonitors::default();
        let mut requires_receipts = false;

        // Receipt-specific fields that are only available from transaction receipts
        let receipt_fields: HashSet<String> = [
            "tx.gas_used".to_string(),
            "tx.status".to_string(),
            "tx.effective_gas_price".to_string(),
        ]
        .into_iter()
        .collect();

        for monitor in monitors {
            match compiler.analyze_script(&monitor.filter_script) {
                Ok(analysis) => {
                    // Check if this monitor's script needs receipt data
                    if !requires_receipts
                        && !analysis.accessed_variables.is_disjoint(&receipt_fields)
                    {
                        requires_receipts = true;
                    }

                    if analysis.accesses_log_variable {
                        if let Some(address_str) = &monitor.address {
                            if address_str.eq_ignore_ascii_case("all") {
                                // Global log-aware monitor
                                organized_monitors
                                    .log_aware_monitors
                                    .entry(GLOBAL_MONITORS_KEY.to_string())
                                    .or_default()
                                    .push(monitor.clone());
                            } else {
                                let address: Address = address_str
                                    .parse()
                                    .expect("Address should be valid at this stage");
                                let checksummed_address = address.to_checksum(None);
                                organized_monitors
                                    .log_aware_monitors
                                    .entry(checksummed_address)
                                    .or_default()
                                    .push(monitor.clone());
                            }
                        } else {
                            // Global log-aware monitor
                            organized_monitors
                                .log_aware_monitors
                                .entry(GLOBAL_MONITORS_KEY.to_string())
                                .or_default()
                                .push(monitor.clone());
                        }
                    } else {
                        organized_monitors.transaction_only_monitors.push(monitor.clone());
                    }
                }
                Err(e) => {
                    // If a script fails to compile, we can't analyze it. Log an error.
                    tracing::error!(monitor_id = monitor.id, error = ?e, "Failed to compile and analyze script during monitor organization");
                }
            }
        }

        (organized_monitors, requires_receipts)
    }

    /// Builds the `InterestRegistry` from the organized monitors.
    /// This includes collecting monitored addresses and global event
    /// signatures.
    fn build_interest_registry(
        organized_monitors: &OrganizedMonitors,
        abi_service: &Arc<AbiService>,
    ) -> InterestRegistry {
        let monitored_addresses: HashSet<Address> = organized_monitors
            .log_aware_monitors
            .iter()
            .filter_map(|entry| entry.key().parse::<Address>().ok())
            .collect();

        let mut global_event_signatures = HashSet::<B256>::new();
        if let Some(global_monitors) =
            organized_monitors.log_aware_monitors.get(GLOBAL_MONITORS_KEY)
        {
            for monitor in global_monitors.value() {
                if let Some(abi_name) = &monitor.abi
                    && let Some(abi) = abi_service.get_abi_by_name(abi_name)
                {
                    for event in abi.events.values().flatten() {
                        global_event_signatures.insert(event.selector());
                    }
                }
            }
        }

        InterestRegistry {
            addresses: Arc::new(monitored_addresses),
            global_event_signatures: Arc::new(global_event_signatures),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use alloy::primitives::{address, b256};
    use tempfile::tempdir;

    use super::*;
    use crate::{
        config::RhaiConfig,
        test_helpers::{LogBuilder, MonitorBuilder, create_test_abi_service, simple_abi_json},
    };

    fn setup() -> (Arc<RhaiCompiler>, Arc<AbiService>) {
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config));
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[]);
        (compiler, abi_service)
    }

    #[test]
    fn test_organize_monitors_categorization() {
        let (compiler, abi_service) = setup();

        let tx_monitor = MonitorBuilder::new().id(1).filter_script("tx.value > 100").build();
        let log_monitor_address = MonitorBuilder::new()
            .id(2)
            .address("0x0000000000000000000000000000000000000001")
            .filter_script(r#"log.name == "Transfer""#)
            .build();
        let log_monitor_global =
            MonitorBuilder::new().id(3).filter_script(r#"log.name == "Approval""#).build();
        let log_monitor_global_all = MonitorBuilder::new()
            .id(4)
            .address("all")
            .filter_script(r#"log.name == "Approval""#)
            .build();

        let monitors = vec![
            tx_monitor.clone(),
            log_monitor_address.clone(),
            log_monitor_global.clone(),
            log_monitor_global_all.clone(),
        ];

        let manager = MonitorManager::new(monitors, compiler, abi_service);
        let snapshot = manager.load();

        // Transaction-only monitors
        assert_eq!(snapshot.organized_monitors.transaction_only_monitors.len(), 1);
        assert_eq!(snapshot.organized_monitors.transaction_only_monitors[0].id, 1);

        // Log-aware monitors
        assert_eq!(snapshot.organized_monitors.log_aware_monitors.len(), 2);

        // Address-specific log monitor
        let addr = address!("0000000000000000000000000000000000000001");
        let addr_checksum = addr.to_checksum(None);
        let address_monitors =
            snapshot.organized_monitors.log_aware_monitors.get(&addr_checksum).unwrap();
        assert_eq!(address_monitors.len(), 1);
        assert_eq!(address_monitors[0].id, 2);

        // Global log monitors
        let global_monitors =
            snapshot.organized_monitors.log_aware_monitors.get(GLOBAL_MONITORS_KEY).unwrap();
        assert_eq!(global_monitors.len(), 2);
        let global_ids: HashSet<i64> = global_monitors.iter().map(|m| m.id).collect();
        assert!(global_ids.contains(&3));
        assert!(global_ids.contains(&4));
    }

    #[test]
    fn test_organize_monitors_requires_receipts() {
        let (compiler, _) = setup();

        let monitors = vec![
            MonitorBuilder::new().id(1).filter_script("tx.gas_used > 1000").build(),
            MonitorBuilder::new().id(2).filter_script("tx.value > 100").build(),
        ];
        let (_, requires_receipts) = MonitorManager::organize_monitors(&monitors, &compiler);
        assert!(requires_receipts, "Should require receipts if 'tx.gas_used' is accessed");

        let monitors = vec![MonitorBuilder::new().id(1).filter_script("tx.status == 1").build()];
        let (_, requires_receipts) = MonitorManager::organize_monitors(&monitors, &compiler);
        assert!(requires_receipts, "Should require receipts if 'tx.status' is accessed");

        let monitors =
            vec![MonitorBuilder::new().id(1).filter_script("tx.effective_gas_price > 100").build()];
        let (_, requires_receipts) = MonitorManager::organize_monitors(&monitors, &compiler);
        assert!(
            requires_receipts,
            "Should require receipts if 'tx.effective_gas_price' is accessed"
        );

        let monitors = vec![MonitorBuilder::new().id(1).filter_script("tx.value > 100").build()];
        let (_, requires_receipts) = MonitorManager::organize_monitors(&monitors, &compiler);
        assert!(!requires_receipts, "Should not require receipts for tx fields in transaction");
    }

    #[test]
    fn test_build_interest_registry() {
        let (compiler, _) = setup();
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("erc20", simple_abi_json())]);

        let monitored_address = address!("0000000000000000000000000000000000000001");

        let log_monitor_address = MonitorBuilder::new()
            .id(1)
            .address(&monitored_address.to_string())
            .filter_script(r#"log.name == "SomeEvent""#)
            .build();

        let global_monitor_with_abi = MonitorBuilder::new()
            .id(2)
            .filter_script(r#"log.name == "Transfer""#)
            .abi("erc20")
            .build();

        let monitors = vec![log_monitor_address, global_monitor_with_abi];
        let (organized_monitors, _) = MonitorManager::organize_monitors(&monitors, &compiler);
        let registry = MonitorManager::build_interest_registry(&organized_monitors, &abi_service);

        // Check addresses
        assert_eq!(registry.addresses.len(), 1);
        assert!(registry.addresses.contains(&monitored_address));

        // Check global event signatures
        assert_eq!(registry.global_event_signatures.len(), 1);
        let transfer_event_signature =
            b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef");
        assert!(registry.global_event_signatures.contains(&transfer_event_signature));
    }

    #[test]
    fn test_is_log_interesting() {
        let monitored_address = address!("0000000000000000000000000000000000000001");
        let other_address = address!("0000000000000000000000000000000000000002");
        let monitored_event_sig =
            b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef");
        let other_event_sig =
            b256!("8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b925");

        let registry = InterestRegistry {
            addresses: Arc::new(HashSet::from([monitored_address])),
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
    fn test_update_monitors() {
        let (compiler, abi_service) = setup();

        let initial_monitors =
            vec![MonitorBuilder::new().id(1).filter_script("tx.value > 100").build()];
        let manager = MonitorManager::new(initial_monitors, compiler.clone(), abi_service.clone());

        // Check initial state
        let snapshot1 = manager.load();
        assert_eq!(snapshot1.organized_monitors.transaction_only_monitors.len(), 1);
        assert_eq!(snapshot1.organized_monitors.transaction_only_monitors[0].id, 1);
        assert!(snapshot1.organized_monitors.log_aware_monitors.is_empty());
        drop(snapshot1);

        // Update with new monitors
        let updated_monitors = vec![
            MonitorBuilder::new()
                .id(2)
                .address("0x0000000000000000000000000000000000000001")
                .filter_script(r#"log.name == "Transfer""#)
                .build(),
            MonitorBuilder::new().id(3).filter_script("tx.value > 200").build(),
        ];
        manager.update(updated_monitors);

        // Check updated state
        let snapshot2 = manager.load();
        assert_eq!(snapshot2.organized_monitors.transaction_only_monitors.len(), 1);
        assert_eq!(snapshot2.organized_monitors.transaction_only_monitors[0].id, 3);
        assert_eq!(snapshot2.organized_monitors.log_aware_monitors.len(), 1);
        let addr = address!("0000000000000000000000000000000000000001");
        let addr_checksum = addr.to_checksum(None);
        assert!(snapshot2.organized_monitors.log_aware_monitors.contains_key(&addr_checksum));
    }

    #[test]
    fn test_empty_monitor_list() {
        let (compiler, abi_service) = setup();

        // Test initialization with an empty list
        let manager = MonitorManager::new(vec![], compiler.clone(), abi_service.clone());
        let snapshot = manager.load();

        assert!(snapshot.organized_monitors.transaction_only_monitors.is_empty());
        assert!(snapshot.organized_monitors.log_aware_monitors.is_empty());
        assert!(!snapshot.requires_receipts);
        assert!(snapshot.interest_registry.addresses.is_empty());
        assert!(snapshot.interest_registry.global_event_signatures.is_empty());
        drop(snapshot);

        // Test updating with an empty list
        manager.update(vec![]);
        let updated_snapshot = manager.load();
        assert!(updated_snapshot.organized_monitors.transaction_only_monitors.is_empty());
        assert!(updated_snapshot.organized_monitors.log_aware_monitors.is_empty());
        assert!(!updated_snapshot.requires_receipts);
        assert!(updated_snapshot.interest_registry.addresses.is_empty());
        assert!(updated_snapshot.interest_registry.global_event_signatures.is_empty());
    }

    #[test]
    fn test_global_monitor_with_missing_abi() {
        let (compiler, abi_service) = setup(); // abi_service is empty

        let monitor = MonitorBuilder::new()
            .id(1)
            .filter_script(r#"log.name == "Transfer""#)
            .abi("non_existent_abi")
            .build();

        let manager = MonitorManager::new(vec![monitor], compiler, abi_service);
        let snapshot = manager.load();

        // The monitor should be organized as a global log-aware monitor.
        assert_eq!(snapshot.organized_monitors.log_aware_monitors.len(), 1);
        let global_monitors =
            snapshot.organized_monitors.log_aware_monitors.get(GLOBAL_MONITORS_KEY).unwrap();
        assert_eq!(global_monitors.len(), 1);

        // No event signatures should be added because the ABI was not found.
        assert!(snapshot.interest_registry.global_event_signatures.is_empty());
    }
}
