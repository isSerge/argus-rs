//! This module defines the `FilteringEngine` trait and its implementations.

use async_trait::async_trait;
use crate::models::monitor_match::MonitorMatch;
use crate::models::CorrelatedBlockItem;

/// A trait for an engine that applies filtering logic to block data.
#[async_trait]
pub trait FilteringEngine {
    /// Evaluates the provided correlated block item against configured monitor rules.
    /// Returns a vector of `MonitorMatch` if any conditions are met.
    async fn evaluate_item(
        &self,
        item: &CorrelatedBlockItem<'_>,
    ) -> Result<Vec<MonitorMatch>, Box<dyn std::error::Error>>;
}

/// A dummy implementation of `FilteringEngine` for initial development.
#[derive(Debug, Default)]
pub struct DummyFilteringEngine;

#[async_trait]
impl FilteringEngine for DummyFilteringEngine {
    async fn evaluate_item(
        &self,
        _item: &CorrelatedBlockItem<'_>,
    ) -> Result<Vec<MonitorMatch>, Box<dyn std::error::Error>> {
        // Placeholder: In a real implementation, this would apply complex filtering logic.
        tracing::debug!("DummyFilteringEngine: Evaluating correlated block item (no actual filtering).");
        Ok(vec![])
    }
}
