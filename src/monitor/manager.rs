use std::{collections::HashSet, sync::Arc};

use alloy::primitives::Address;
use arc_swap::{ArcSwap, Guard};
use dashmap::DashMap;

use super::InterestRegistry;
use crate::{abi::AbiService, engine::rhai::RhaiCompiler, models::monitor::Monitor};

/// The key used to store global log-aware monitors in the
/// `address_specific_monitors` map.
pub const GLOBAL_MONITORS_KEY: &str = "*";

/// A container for monitors that have been organized for efficient execution.
#[derive(Debug, Default)]
pub struct OrganizedMonitors {
    /// Monitors that run on every transaction and do not depend on log data.
    pub global_tx_monitors: Vec<Monitor>,
    /// Monitors that run on every log, regardless of its source address.
    pub global_log_monitors: Vec<Monitor>,
    /// Monitors that are tied to a specific contract address. These can be
    /// either transaction-focused or log-focused.
    pub address_specific_monitors: DashMap<String, Vec<Monitor>>,
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
        tracing::debug!("Organizing assets for {} monitors", monitors.len());
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

        let mut log_addresses = HashSet::new();
        let mut calldata_addresses = HashSet::new();

        for monitor in monitors {
            let analysis = match compiler.analyze_script(&monitor.filter_script) {
                Ok(analysis) => analysis,
                Err(err) => {
                    tracing::error!(
                        "Failed to analyze filter script for monitor {}: {}. Skipping this \
                         monitor.",
                        monitor.id,
                        err
                    );
                    continue;
                }
            };

            if !requires_receipts && !analysis.accessed_variables.is_disjoint(&receipt_fields) {
                requires_receipts = true;
            }

            let is_log_aware = analysis.accesses_log_variable;
            let has_address = monitor.address.is_some()
                && !monitor.address.as_deref().unwrap_or("").eq_ignore_ascii_case("all");

            if has_address {
                if let Some(address) =
                    monitor.address.as_deref().and_then(|a| a.parse::<Address>().ok())
                {
                    let address_checksum = address.to_checksum(None);
                    organized_monitors
                        .address_specific_monitors
                        .entry(address_checksum)
                        .or_default()
                        .push(monitor.clone());

                    if is_log_aware {
                        log_addresses.insert(address);
                    }
                    if monitor.decode_calldata {
                        calldata_addresses.insert(address);
                    }
                }
            } else {
                // Global monitor
                if is_log_aware {
                    organized_monitors.global_log_monitors.push(monitor.clone());
                } else {
                    organized_monitors.global_tx_monitors.push(monitor.clone());
                }
            }
        }

        // Global monitors
        let mut global_event_signatures = HashSet::new();
        for monitor in &organized_monitors.global_log_monitors {
            if let Some(abi_name) = &monitor.abi {
                if let Some(abi) = abi_service.get_abi_by_name(abi_name) {
                    for event in abi.events.values().flatten() {
                        global_event_signatures.insert(event.selector());
                    }
                }
            }
        }

        tracing::debug!(
            "Created interest registry with {} log addresses, {} calldata addresses, and {} \
             global event signatures",
            log_addresses.len(),
            calldata_addresses.len(),
            global_event_signatures.len()
        );

        let interest_registry = InterestRegistry {
            log_addresses: Arc::new(log_addresses),
            calldata_addresses: Arc::new(calldata_addresses),
            global_event_signatures: Arc::new(global_event_signatures),
        };

        tracing::debug!(
            "Organized monitors:
            {} transaction-only, 
            {} global log-aware,
            {} address-specific groups
            ",
            organized_monitors.global_tx_monitors.len(),
            organized_monitors.global_log_monitors.len(),
            organized_monitors.address_specific_monitors.len(),
        );

        MonitorAssetState { organized_monitors, requires_receipts, interest_registry }
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
        test_helpers::{MonitorBuilder, create_test_abi_service, simple_abi_json},
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

        // Global transaction-only monitors
        assert_eq!(snapshot.organized_monitors.global_tx_monitors.len(), 1);
        assert_eq!(snapshot.organized_monitors.global_tx_monitors[0].id, 1);

        // Address-specific monitors
        assert_eq!(snapshot.organized_monitors.address_specific_monitors.len(), 1);
        let addr = address!("0000000000000000000000000000000000000001");
        let addr_checksum = addr.to_checksum(None);
        let address_monitors =
            snapshot.organized_monitors.address_specific_monitors.get(&addr_checksum).unwrap();
        assert_eq!(address_monitors.len(), 1);
        assert_eq!(address_monitors[0].id, 2);

        // Global log monitors
        assert_eq!(snapshot.organized_monitors.global_log_monitors.len(), 2);
        let global_log_ids: HashSet<i64> =
            snapshot.organized_monitors.global_log_monitors.iter().map(|m| m.id).collect();
        assert!(global_log_ids.contains(&3));
        assert!(global_log_ids.contains(&4));
    }

    #[test]
    fn test_organize_assets_requires_receipts() {
        let (compiler, abi_service) = setup();

        let monitors = vec![
            MonitorBuilder::new().id(1).filter_script("tx.gas_used > 1000").build(), /* requires receipts */
            MonitorBuilder::new().id(2).filter_script("tx.value > 100").build(),
        ];

        let manager = MonitorManager::new(monitors, compiler, abi_service);

        let snapshot = manager.load();

        assert!(snapshot.requires_receipts);
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

        let manager = MonitorManager::new(monitors, compiler, abi_service);

        let snapshot = manager.load();

        // Check log addresses
        assert_eq!(snapshot.interest_registry.log_addresses.len(), 1);
        assert!(snapshot.interest_registry.log_addresses.contains(&monitored_address));
    }

    #[test]
    fn test_update_monitors() {
        let (compiler, abi_service) = setup();

        let initial_monitors =
            vec![MonitorBuilder::new().id(1).filter_script("tx.value > 100").build()];
        let manager = MonitorManager::new(initial_monitors, compiler.clone(), abi_service.clone());

        // Check initial state
        let snapshot1 = manager.load();
        assert_eq!(snapshot1.organized_monitors.global_tx_monitors.len(), 1);
        assert_eq!(snapshot1.organized_monitors.global_tx_monitors[0].id, 1);
        assert!(snapshot1.organized_monitors.address_specific_monitors.is_empty());
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
        assert_eq!(snapshot2.organized_monitors.global_tx_monitors.len(), 1);
        assert_eq!(snapshot2.organized_monitors.global_tx_monitors[0].id, 3);
        assert_eq!(snapshot2.organized_monitors.address_specific_monitors.len(), 1);
        let addr = address!("0000000000000000000000000000000000000001");
        let addr_checksum = addr.to_checksum(None);
        assert!(
            snapshot2.organized_monitors.address_specific_monitors.contains_key(&addr_checksum)
        );
    }

    #[test]
    fn test_update_rebuilds_interest_registry_correctly() {
        let (compiler, abi_service) = setup();

        // Initial state: One address-specific log monitor
        let initial_monitors = vec![
            MonitorBuilder::new()
                .id(1)
                .address("0x1111111111111111111111111111111111111111")
                .filter_script("log.name == \"A\"")
                .build(),
        ];
        let manager = MonitorManager::new(initial_monitors, compiler.clone(), abi_service.clone());

        let snapshot1 = manager.load();
        assert_eq!(snapshot1.interest_registry.log_addresses.len(), 1);
        assert!(
            snapshot1
                .interest_registry
                .log_addresses
                .contains(&address!("1111111111111111111111111111111111111111"))
        );
        assert!(snapshot1.interest_registry.global_event_signatures.is_empty());
        drop(snapshot1);

        // New state: A different address and a new global monitor
        let updated_monitors = vec![
            MonitorBuilder::new()
                .id(2)
                .address("0x2222222222222222222222222222222222222222")
                .filter_script("log.name == \"B\"")
                .build(),
            MonitorBuilder::new()
                .id(3)
                .address("all")
                .abi("simple")
                .filter_script("log.name == \"Transfer\"")
                .build(),
        ];

        let temp_dir = tempdir().unwrap();
        let (abi_service_with_abi, _) =
            create_test_abi_service(&temp_dir, &[("simple", simple_abi_json())]);
        let manager = MonitorManager::new(vec![], compiler.clone(), abi_service_with_abi);

        manager.update(updated_monitors);

        let snapshot2 = manager.load();
        // Address set should be completely replaced, not merged.
        assert_eq!(snapshot2.interest_registry.log_addresses.len(), 1);
        assert!(
            !snapshot2
                .interest_registry
                .log_addresses
                .contains(&address!("1111111111111111111111111111111111111111"))
        );
        assert!(
            snapshot2
                .interest_registry
                .log_addresses
                .contains(&address!("2222222222222222222222222222222222222222"))
        );

        // Global signatures should now be populated.
        assert_eq!(snapshot2.interest_registry.global_event_signatures.len(), 1);
        let transfer_event_sig =
            b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef");
        assert!(snapshot2.interest_registry.global_event_signatures.contains(&transfer_event_sig));
    }

    #[test]
    fn test_empty_monitor_list() {
        let (compiler, abi_service) = setup();

        // Test initialization with an empty list
        let manager = MonitorManager::new(vec![], compiler.clone(), abi_service.clone());
        let snapshot = manager.load();

        assert!(snapshot.organized_monitors.global_tx_monitors.is_empty());
        assert!(snapshot.organized_monitors.global_log_monitors.is_empty());
        assert!(snapshot.organized_monitors.address_specific_monitors.is_empty());
        assert!(!snapshot.requires_receipts);
        assert!(snapshot.interest_registry.log_addresses.is_empty());
        assert!(snapshot.interest_registry.global_event_signatures.is_empty());
        drop(snapshot);

        // Test updating with an empty list
        manager.update(vec![]);
        let updated_snapshot = manager.load();
        assert!(updated_snapshot.organized_monitors.global_tx_monitors.is_empty());
        assert!(updated_snapshot.organized_monitors.global_log_monitors.is_empty());
        assert!(updated_snapshot.organized_monitors.address_specific_monitors.is_empty());
        assert!(!updated_snapshot.requires_receipts);
        assert!(updated_snapshot.interest_registry.log_addresses.is_empty());
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
        assert_eq!(snapshot.organized_monitors.global_log_monitors.len(), 1);
        assert!(snapshot.organized_monitors.address_specific_monitors.is_empty());

        // No event signatures should be added because the ABI was not found.
        assert!(snapshot.interest_registry.global_event_signatures.is_empty());
    }

    #[test]
    fn test_organize_assets_calldata_aware() {
        let (compiler, abi_service) = setup();

        let calldata_aware_address = address!("0000000000000000000000000000000000000001");

        let monitors = vec![
            MonitorBuilder::new()
                .id(1)
                .address(&calldata_aware_address.to_string())
                .decode_calldata(true)
                .filter_script("true") // Just a placeholder script
                .build(),
            MonitorBuilder::new()
                .id(2)
                .address("0x0000000000000000000000000000000000000002")
                .decode_calldata(false)
                .filter_script("tx.value > 0")
                .build(),
        ];

        let manager = MonitorManager::new(monitors, compiler, abi_service);
        let snapshot = manager.load();

        assert_eq!(snapshot.interest_registry.calldata_addresses.len(), 1);
        assert!(snapshot.interest_registry.calldata_addresses.contains(&calldata_aware_address));
    }

    #[test]
    fn test_build_interest_registry_aggregates_global_signatures() {
        let (compiler, _) = setup();
        let temp_dir = tempdir().unwrap();
        let weth_abi = r#"[
        {
            "type":"event",
            "name":"Deposit",
            "inputs":[
                {"name":"dst","type":"address","indexed":true},
                {"name":"wad","type":"uint256","indexed":false}
            ],
            "anonymous":false
        }
    ]"#;
        let (abi_service, _) = create_test_abi_service(
            &temp_dir,
            &[("simple", simple_abi_json()), ("weth", weth_abi)],
        );

        let global_erc20_monitor = MonitorBuilder::new()
            .id(1)
            .address("all")
            .abi("simple")
            .filter_script("log.name == \"Transfer\"")
            .build();

        let global_weth_monitor = MonitorBuilder::new()
            .id(2)
            .address("all")
            .abi("weth")
            .filter_script("log.name == \"Deposit\"")
            .build();

        let monitors = vec![global_erc20_monitor, global_weth_monitor];

        let manager = MonitorManager::new(monitors, compiler, abi_service);

        let snapshot = manager.load();
        let registry = &snapshot.interest_registry;

        // Should contain event signatures from BOTH ABIs.
        assert_eq!(registry.global_event_signatures.len(), 2);

        // Keccak256 hash of "Transfer(address,address,uint256)"
        let transfer_sig =
            b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef");
        // Keccak256 hash of "Deposit()"
        let deposit_sig = b256!("e1fffcc4923d04b559f4d29a8bfc6cda04eb5b0d3c460751c2402c5c5cc9109c");

        assert!(registry.global_event_signatures.contains(&transfer_sig));
        assert!(registry.global_event_signatures.contains(&deposit_sig));
    }
}
