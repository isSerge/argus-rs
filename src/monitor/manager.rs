//! This module is responsible for classifying monitors, analyzing their
//! scripts, and maintaining the overall state of the monitoring system.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use arc_swap::{ArcSwap, Guard};
use bitflags::bitflags;

use super::{InterestRegistry, InterestRegistryBuilder};
use crate::{
    abi::AbiService,
    engine::rhai::{RhaiCompiler, ScriptAnalysis},
    models::monitor::Monitor,
};

const TX_GAS_USED: &str = "tx.gas_used";
const TX_STATUS: &str = "tx.status";
const TX_EFFECTIVE_GAS_PRICE: &str = "tx.effective_gas_price";

/// A monitor along with its capabilities and script analysis.
#[derive(Debug, Clone)]
pub struct ClassifiedMonitor {
    /// The original monitor definition.
    pub monitor: Arc<Monitor>,
    /// The capabilities of this monitor based on its script analysis.
    pub caps: MonitorCapabilities,
    /// The detailed analysis of the monitor's filter script.
    pub analysis: ScriptAnalysis,
}

bitflags! {
    /// Capabilities that a monitor's script accesses.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct MonitorCapabilities: u8 {
        /// Script accesses `tx.*`
        const TX   = 0b0000_0001;
        /// Script accesses `log.*`
        const LOG  = 0b0000_0010;
        /// Script accesses `decoded_call.*`
        const CALL = 0b0000_0100;
    }
}

/// Represents the state of all monitors, including their capabilities and
/// script analysis.
#[derive(Debug, Default)]
pub struct MonitorAssetState {
    /// A mapping of monitor IDs to their classified representations.
    pub monitors_by_id: HashMap<i64, ClassifiedMonitor>,

    /// IDs of all monitors that are transaction-aware (pure tx/call or hybrid).
    pub tx_aware_monitors: Vec<i64>,

    /// IDs of all monitors that are log-aware (need to be evaluated against
    /// logs).
    pub log_aware_monitors: Vec<i64>,

    /// An optimized, fast-lookup registry for the BlockProcessor.
    pub interest_registry: InterestRegistry,

    /// The flag indicating if any monitor in this snapshot requires receipts.
    pub requires_receipts: bool,

    /// The flag indicating if any monitor is purely transaction-based (used for
    /// filtering optimizations).
    pub has_transaction_only_monitors: bool,
}

/// Manages monitors, including organizing them for efficient execution and
/// providing atomic updates to the monitor state.
#[derive(Debug)]
pub struct MonitorManager {
    /// The Rhai script compiler used for analyzing monitor scripts.
    compiler: Arc<RhaiCompiler>,
    /// The ABI service used for resolving ABIs in monitors.
    abi_service: Arc<AbiService>,
    /// The current state of monitors, wrapped in an `ArcSwap` for atomic
    /// updates.
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
        let initial_state = Self::organize_assets(initial_monitors, &compiler, &abi_service);
        Self { compiler, abi_service, state: ArcSwap::new(Arc::new(initial_state)) }
    }

    /// Updates the monitor state with a new set of monitors.
    /// This method atomically swaps the entire monitor state.
    pub fn update(&self, monitors: Vec<Monitor>) {
        let state = Self::organize_assets(monitors, &self.compiler, &self.abi_service);
        self.state.store(Arc::new(state));
    }

    /// Loads the current snapshot of the monitor state.
    pub fn load(&self) -> Guard<Arc<MonitorAssetState>> {
        self.state.load()
    }

    /// Organizes monitors into the `MonitorAssetState`, categorizing them and
    /// building the interest registry.
    fn organize_assets(
        monitors: Vec<Monitor>,
        compiler: &Arc<RhaiCompiler>,
        abi_service: &Arc<AbiService>,
    ) -> MonitorAssetState {
        tracing::debug!("Organizing assets for {} monitors", monitors.len());

        let (classified, failed): (Vec<_>, Vec<_>) = monitors
            .into_iter()
            .map(|m| Self::classify_monitor(compiler, m))
            .partition(Result::is_ok);

        let classified_monitors =
            classified.into_iter().map(Result::unwrap).collect::<Vec<(ClassifiedMonitor, bool)>>();

        if !failed.is_empty() {
            tracing::warn!(
                count = failed.len(),
                "Failed to classify {} monitors due to script analysis errors.",
                failed.len()
            );
        }

        let requires_receipts = classified_monitors.iter().any(|(_, needs_receipt)| *needs_receipt);

        // Build interest registry by borrowing from classified_monitors
        let monitors_for_registry: Vec<&ClassifiedMonitor> =
            classified_monitors.iter().map(|(cm, _)| cm).collect();
        let interest_registry = Self::build_interest_registry(&monitors_for_registry, abi_service);

        let has_transaction_only_monitors =
            classified_monitors.iter().any(|(cm, _)| cm.caps == MonitorCapabilities::TX);

        let mut monitors_by_id = HashMap::new();
        let mut tx_aware_monitors = Vec::new();
        let mut log_aware_monitors = Vec::new();

        // Consume classified_monitors to avoid cloning
        for (cm, _) in classified_monitors {
            let monitor_id = cm.monitor.id;

            let is_tx_aware = cm.caps.contains(MonitorCapabilities::TX)
                || cm.caps.contains(MonitorCapabilities::CALL);
            let is_log_aware = cm.caps.contains(MonitorCapabilities::LOG);

            if is_tx_aware {
                tx_aware_monitors.push(monitor_id);
            }
            if is_log_aware {
                log_aware_monitors.push(monitor_id);
            }
            // Handle monitors like `true` which have empty caps
            if !is_tx_aware && !is_log_aware {
                tx_aware_monitors.push(monitor_id);
            }

            // Move cm into the HashMap instead of cloning
            monitors_by_id.insert(monitor_id, cm);
        }

        tracing::debug!(
            "Organized monitors: {} total, requires_receipts={}, has_tx_only_monitors={}",
            monitors_by_id.len(),
            requires_receipts,
            has_transaction_only_monitors
        );

        MonitorAssetState {
            monitors_by_id,
            tx_aware_monitors,
            log_aware_monitors,
            requires_receipts,
            interest_registry,
            has_transaction_only_monitors,
        }
    }

    /// Classifies a single monitor by analyzing its script and determining its
    /// capabilities.
    /// Returns the classified monitor and a flag indicating if it requires
    /// receipts.
    fn classify_monitor(
        compiler: &Arc<RhaiCompiler>,
        monitor: Monitor,
    ) -> Result<(ClassifiedMonitor, bool), Box<dyn std::error::Error>> {
        // Analyze the filter script
        let analysis = compiler.analyze_script(&monitor.filter_script)?;

        // Receipt-specific fields that are only available from transaction receipts
        let receipt_fields: HashSet<String> =
            [TX_GAS_USED.to_string(), TX_STATUS.to_string(), TX_EFFECTIVE_GAS_PRICE.to_string()]
                .into_iter()
                .collect();

        let needs_receipt =
            analysis.accessed_variables.iter().any(|v| receipt_fields.contains(v.as_str()));

        // Determine capabilities based on accessed variables
        let mut caps = MonitorCapabilities::empty();

        // Check if any tx fields are accessed
        if analysis.accessed_variables.iter().any(|var| var.starts_with("tx.")) {
            caps |= MonitorCapabilities::TX;
        }

        // Check if any log fields are accessed
        if analysis.accesses_log_variable {
            caps |= MonitorCapabilities::LOG;
        }

        // Check if any decoded_call fields are accessed
        if analysis
            .accessed_variables
            .iter()
            .any(|var| var.starts_with("decoded_call.") || var == "decoded_call")
        {
            caps |= MonitorCapabilities::CALL;
        }

        // If a script doesn't access any specific context, it's treated as a
        // transaction-level monitor by default.
        if caps.is_empty() {
            caps = MonitorCapabilities::TX;
        }

        let cm = ClassifiedMonitor { monitor: Arc::new(monitor), caps, analysis };

        Ok((cm, needs_receipt))
    }

    /// Builds the InterestRegistry from the classified monitors.
    /// This method analyzes each monitor's capabilities and ABI to
    /// determine the addresses and event signatures of interest.
    fn build_interest_registry(
        classified_monitors: &[&ClassifiedMonitor],
        abi_service: &Arc<AbiService>,
    ) -> InterestRegistry {
        classified_monitors
            .iter()
            .fold(InterestRegistryBuilder::default(), |mut builder, cm| {
                builder.add(cm, abi_service);
                builder
            })
            .build()
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::{address, b256};
    use tempfile::tempdir;

    use super::*;
    use crate::{
        config::RhaiConfig,
        test_helpers::{MonitorBuilder, create_test_abi_service, erc20_abi_json},
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
        let address = address!("0000000000000000000000000000000000000001");
        let (compiler, _) = setup();
        let temp_dir = tempdir().unwrap();
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json())]);
        abi_service.link_abi(address, "erc20").unwrap();

        let monitors = vec![
            // TX only
            MonitorBuilder::new().id(1).filter_script("tx.value > 100").build(),
            // LOG only
            MonitorBuilder::new()
                .id(2)
                .filter_script("log.name == \"Transfer\"")
                .abi("erc20")
                .address(address.to_string().as_str())
                .build(),
            // CALL only
            MonitorBuilder::new().id(3).filter_script("decoded_call.name == \"approve\"").build(),
            // TX and LOG
            MonitorBuilder::new()
                .id(4)
                .filter_script("tx.value > 100 && log.name == \"Transfer\"")
                .abi("erc20")
                .address(address.to_string().as_str())
                .build(),
            // TX and CALL
            MonitorBuilder::new()
                .id(5)
                .filter_script("tx.value > 100 && decoded_call.name == \"approve\"")
                .build(),
            // LOG and CALL
            MonitorBuilder::new()
                .id(6)
                .filter_script("log.name == \"Transfer\" && decoded_call.name == \"approve\"")
                .abi("erc20")
                .address(address.to_string().as_str())
                .build(),
            // TX, LOG, and CALL
            MonitorBuilder::new()
                .id(7)
                .filter_script(
                    "tx.value > 100 && log.name == \"Transfer\" && decoded_call.name == \
                     \"Transfer\"",
                )
                .abi("erc20")
                .address(address.to_string().as_str())
                .build(),
            // No context access (should default to TX)
            MonitorBuilder::new().id(8).filter_script("true").build(),
            // Accessing decoded_call directly (should be CALL)
            MonitorBuilder::new().id(9).filter_script("decoded_call == ()").build(),
        ];

        let manager = MonitorManager::new(monitors, compiler, abi_service);
        let snapshot = manager.load();

        let get_caps =
            |id: i64| -> MonitorCapabilities { snapshot.monitors_by_id.get(&id).unwrap().caps };

        assert_eq!(get_caps(1), MonitorCapabilities::TX);
        assert_eq!(get_caps(2), MonitorCapabilities::LOG);
        assert_eq!(get_caps(3), MonitorCapabilities::CALL);
        assert_eq!(get_caps(4), MonitorCapabilities::TX | MonitorCapabilities::LOG);
        assert_eq!(get_caps(5), MonitorCapabilities::TX | MonitorCapabilities::CALL);
        assert_eq!(get_caps(6), MonitorCapabilities::LOG | MonitorCapabilities::CALL);
        assert_eq!(
            get_caps(7),
            MonitorCapabilities::TX | MonitorCapabilities::LOG | MonitorCapabilities::CALL
        );
        assert_eq!(get_caps(8), MonitorCapabilities::TX);
        assert_eq!(get_caps(9), MonitorCapabilities::CALL);
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
        let (abi_service, _) = create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json())]);

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

        // Check log interests
        assert_eq!(snapshot.interest_registry.log_interests.len(), 1);
        assert!(snapshot.interest_registry.log_interests.contains_key(&monitored_address));
    }

    #[test]
    fn test_update_monitors() {
        let (compiler, abi_service) = setup();
        let manager = MonitorManager::new(vec![], compiler.clone(), abi_service.clone());

        // Initial state: empty
        let snapshot1 = manager.load();
        assert!(snapshot1.monitors_by_id.is_empty());
        drop(snapshot1);

        // Update with new monitors
        let monitors = vec![
            MonitorBuilder::new().id(1).filter_script("tx.value > 100").build(),
            MonitorBuilder::new().id(2).filter_script("true").build(),
        ];
        manager.update(monitors);

        // Final state: updated
        let snapshot2 = manager.load();
        assert_eq!(snapshot2.monitors_by_id.len(), 2);
        assert!(snapshot2.monitors_by_id.contains_key(&1));
        assert!(snapshot2.monitors_by_id.contains_key(&2));
        assert_eq!(snapshot2.monitors_by_id.get(&1).unwrap().monitor.id, 1);
        assert_eq!(snapshot2.monitors_by_id.get(&2).unwrap().monitor.id, 2);
    }

    #[test]
    fn test_update_rebuilds_interest_registry_correctly() {
        let (compiler, abi_service) = setup();
        let address1 = address!("1111111111111111111111111111111111111111");
        let address2 = address!("2222222222222222222222222222222222222222");

        // Initial state: One address-specific log monitor
        let initial_monitors = vec![
            MonitorBuilder::new()
                .id(1)
                .address(&address1.to_checksum(None))
                .filter_script("log.name == \"A\"")
                .build(),
        ];
        let manager = MonitorManager::new(initial_monitors, compiler.clone(), abi_service.clone());

        let snapshot1 = manager.load();
        assert_eq!(snapshot1.interest_registry.log_interests.len(), 1);
        assert!(snapshot1.interest_registry.log_interests.contains_key(&address1));
        assert!(snapshot1.interest_registry.global_event_signatures.is_empty());
        drop(snapshot1);

        // New state: A different address and a new global monitor
        let updated_monitors = vec![
            MonitorBuilder::new()
                .id(2)
                .address(&address2.to_checksum(None))
                .filter_script("log.name == \"B\"")
                .build(),
            MonitorBuilder::new()
                .id(3)
                .address("all")
                .abi("erc20")
                .filter_script("log.name == \"Transfer\"")
                .build(),
        ];

        let temp_dir = tempdir().unwrap();
        let (abi_service_with_abi, _) =
            create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json())]);
        let manager = MonitorManager::new(vec![], compiler.clone(), abi_service_with_abi);

        manager.update(updated_monitors);

        let snapshot2 = manager.load();
        // Address set should be completely replaced, not merged.
        assert_eq!(snapshot2.interest_registry.log_interests.len(), 1);
        assert!(!snapshot2.interest_registry.log_interests.contains_key(&address1));
        assert!(snapshot2.interest_registry.log_interests.contains_key(&address2));

        // Global signatures should now be populated with the Transfer event signature,
        // other signatures should be ignored.
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

        assert!(snapshot.monitors_by_id.is_empty());
        assert!(!snapshot.requires_receipts);
        assert!(snapshot.interest_registry.log_interests.is_empty());
        assert!(snapshot.interest_registry.global_event_signatures.is_empty());
        drop(snapshot);

        // Test updating with an empty list
        manager.update(vec![]);
        let updated_snapshot = manager.load();
        assert!(updated_snapshot.monitors_by_id.is_empty());
        assert!(!updated_snapshot.requires_receipts);
        assert!(updated_snapshot.interest_registry.log_interests.is_empty());
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
        assert_eq!(snapshot.monitors_by_id.len(), 1);

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
                .filter_script("decoded_call.name == \"approve\"") // script accesses decoded_call
                .build(),
            MonitorBuilder::new()
                .id(2)
                .address("0x0000000000000000000000000000000000000002")
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
        let (abi_service, _) =
            create_test_abi_service(&temp_dir, &[("erc20", erc20_abi_json()), ("weth", weth_abi)]);

        let global_erc20_monitor = MonitorBuilder::new()
            .id(1)
            .address("all")
            .abi("erc20")
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

        // Should contain only 2 event signatures from BOTH ABIs, it should ignore other
        // signatures that are not relevant for current monitors
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
