use std::sync::Arc;

use alloy::{
    providers::{Provider, ProviderBuilder},
    transports::mock::Asserter,
};

/// Creates a mock provider and an asserter for testing purposes.
pub fn mock_provider() -> (Arc<dyn Provider + Send + Sync>, Asserter) {
    let asserter = Asserter::new();
    let provider = Arc::new(ProviderBuilder::new().connect_mocked_client(asserter.clone()));
    (provider, asserter)
}
