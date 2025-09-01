//! This module provides monitor organization capabilities.

use std::{collections::HashSet, sync::Arc};

use alloy::primitives::Address;
use dashmap::DashMap;

use crate::{engine::rhai::compiler::RhaiCompiler, models::monitor::Monitor};

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

/// Organizes monitors based on script analysis for efficient processing.
#[derive(Debug)]
pub struct MonitorOrganizer {
    compiler: Arc<RhaiCompiler>,
}

impl MonitorOrganizer {
    /// Creates a new `MonitorOrganizer`.
    pub fn new(compiler: Arc<RhaiCompiler>) -> Self {
        Self { compiler }
    }

    /// Organizes monitors into optimized collections based on their script
    /// analysis.
    ///
    /// Returns a tuple containing the organized monitors and a boolean
    /// indicating if any monitor requires transaction receipt data.
    pub fn organize(&self, monitors: Vec<Monitor>) -> (OrganizedMonitors, bool) {
        let mut organized = OrganizedMonitors::default();
        let mut needs_receipts = false;

        // Receipt-specific fields that are only available from transaction receipts
        let receipt_fields: HashSet<String> = [
            "tx.gas_used".to_string(),
            "tx.status".to_string(),
            "tx.effective_gas_price".to_string(),
        ]
        .into_iter()
        .collect();

        for monitor in monitors {
            match self.compiler.analyze_script(&monitor.filter_script) {
                Ok(analysis) => {
                    // Check if this monitor's script needs receipt data
                    if !needs_receipts && !analysis.accessed_variables.is_disjoint(&receipt_fields)
                    {
                        needs_receipts = true;
                    }

                    if analysis.accesses_log_variable {
                        if let Some(address_str) = &monitor.address {
                            if address_str.eq_ignore_ascii_case("all") {
                                // Global log-aware monitor
                                organized
                                    .log_aware_monitors
                                    .entry(GLOBAL_MONITORS_KEY.to_string())
                                    .or_default()
                                    .push(monitor);
                            } else {
                                let address: Address = address_str
                                    .parse()
                                    .expect("Address should be valid at this stage");
                                let checksummed_address = address.to_checksum(None);
                                organized
                                    .log_aware_monitors
                                    .entry(checksummed_address)
                                    .or_default()
                                    .push(monitor);
                            }
                        } else {
                            // Global log-aware monitor
                            organized
                                .log_aware_monitors
                                .entry(GLOBAL_MONITORS_KEY.to_string())
                                .or_default()
                                .push(monitor);
                        }
                    } else {
                        organized.transaction_only_monitors.push(monitor);
                    }
                }
                Err(e) => {
                    // If a script fails to compile, we can't analyze it. Log an error.
                    tracing::error!(monitor_id = monitor.id, error = ?e, "Failed to compile and analyze script during monitor organization");
                }
            }
        }

        (organized, needs_receipts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::RhaiConfig, test_helpers::MonitorBuilder};

    #[test]

    fn test_organize_monitors_categorization() {
        let config = RhaiConfig::default();
        let compiler = Arc::new(RhaiCompiler::new(config.clone()));
        let organizer = MonitorOrganizer::new(compiler);

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

        let (organized, _) = organizer.organize(monitors);

        assert_eq!(organized.transaction_only_monitors.len(), 1);
        assert_eq!(organized.transaction_only_monitors[0].id, 1);

        assert_eq!(organized.log_aware_monitors.len(), 2);
        let addr_checksum = "0x0000000000000000000000000000000000000001".to_string();
        assert_eq!(organized.log_aware_monitors.get(&addr_checksum).unwrap().len(), 1);
        assert_eq!(organized.log_aware_monitors.get(&addr_checksum).unwrap()[0].id, 2);
        assert_eq!(organized.log_aware_monitors.get(GLOBAL_MONITORS_KEY).unwrap().len(), 1);
        assert_eq!(organized.log_aware_monitors.get(GLOBAL_MONITORS_KEY).unwrap()[0].id, 3);
    }
}
