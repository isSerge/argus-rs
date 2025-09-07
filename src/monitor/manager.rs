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
struct InterestRegistry {
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
                .map_or(false, |topic0| self.global_event_signatures.contains(topic0))
    }
}

/// Manages monitors, including organizing them for efficient execution and
/// providing atomic updates to the monitor state.
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
                if let Some(abi_name) = &monitor.abi {
                    if let Some(abi) = abi_service.get_abi_by_name(abi_name) {
                        for event in abi.events.values().flatten() {
                            global_event_signatures.insert(event.selector());
                        }
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
    use alloy::primitives::{address, b256};
    use tempfile::tempdir;

    use super::*;
    use crate::{abi::AbiRepository, config::RhaiConfig, test_helpers::MonitorBuilder};

    fn create_test_abi_service() -> Arc<AbiService> {
        let temp_dir = tempdir().unwrap();
        let abi_repo = Arc::new(AbiRepository::new(temp_dir.path()).unwrap());
        Arc::new(AbiService::new(abi_repo))
    }

    #[test]

    fn test_organize_monitors_categorization() {
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config.clone()));
        let abi_service = create_test_abi_service();

        let tx_monitor = MonitorBuilder::new().id(1).filter_script("tx.value > 100").build();
        let log_monitor_address = MonitorBuilder::new()
            .id(2)
            .address("0x0000000000000000000000000000000000000001")
            .filter_script(r#"log.name == "Transfer""#)
            .build();
        let log_monitor_global =
            MonitorBuilder::new().id(3).filter_script(r#"log.name == "Approval""#).build();

        let monitors =
            vec![tx_monitor.clone(), log_monitor_address.clone(), log_monitor_global.clone()];

        let manager = MonitorManager::new(monitors.clone(), compiler, abi_service);

        let snapshot = manager.load();

        assert_eq!(snapshot.organized_monitors.transaction_only_monitors.len(), 1);
        assert_eq!(snapshot.organized_monitors.transaction_only_monitors[0].id, 1);

        assert_eq!(snapshot.organized_monitors.log_aware_monitors.len(), 2);
        let addr_checksum = "0x0000000000000000000000000000000000000001".to_string();
        assert_eq!(
            snapshot.organized_monitors.log_aware_monitors.get(&addr_checksum).unwrap().len(),
            1
        );
        assert_eq!(
            snapshot.organized_monitors.log_aware_monitors.get(&addr_checksum).unwrap()[0].id,
            2
        );
        assert_eq!(
            snapshot.organized_monitors.log_aware_monitors.get(GLOBAL_MONITORS_KEY).unwrap().len(),
            1
        );
        assert_eq!(
            snapshot.organized_monitors.log_aware_monitors.get(GLOBAL_MONITORS_KEY).unwrap()[0].id,
            3
        );
    }

    #[test]
    fn test_interest_registry() {
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config.clone()));
        let abi_service = create_test_abi_service();

        let tx_monitor = MonitorBuilder::new().id(1).filter_script("tx.value > 100").build();
        let log_monitor_address = MonitorBuilder::new()
            .id(2)
            .address("0x0000000000000000000000000000000000000001")
            .filter_script(r#"log.name == "Transfer""#)
            .build();
        let log_monitor_global =
            MonitorBuilder::new().id(3).filter_script(r#"log.name == "Approval""#).build();

        let monitors =
            vec![tx_monitor.clone(), log_monitor_address.clone(), log_monitor_global.clone()];

        let manager = MonitorManager::new(monitors.clone(), compiler, abi_service);

        let snapshot = manager.load();
        let registry = &snapshot.interest_registry;

        assert_eq!(registry.addresses.len(), 1); // Only one specific address
        assert!(
            registry.addresses.contains(&address!("0x0000000000000000000000000000000000000001"))
        );
        assert_eq!(registry.global_event_signatures.len(), 0); // No ABI, so no event signatures
    }
}
