use std::{collections::HashMap, sync::Arc};

use crate::{
    engine::match_manager::MatchManager,
    http_client::HttpClientPool,
    models::notifier::NotifierConfig,
    notification::NotificationService,
    persistence::{sqlite::SqliteStateRepository, traits::GenericStateRepository},
};

/// A generic helper to create an MatchManager with a provided state repository
/// and no action handler. This is useful for tests that need to mock the state
/// repository or don't require action execution.
pub fn create_test_match_manager_with_repo<T: GenericStateRepository + Send + Sync + 'static>(
    notifiers: Arc<HashMap<String, NotifierConfig>>,
    state_repo: Arc<T>,
) -> MatchManager<T> {
    let client_pool = Arc::new(HttpClientPool::default());
    let notification_service = Arc::new(NotificationService::new(notifiers.clone(), client_pool));
    MatchManager::new(notification_service, state_repo, notifiers, None)
}

/// A helper function to create an MatchManager with an in-memory
/// SqliteStateRepository.
pub async fn create_test_match_manager(
    notifiers: Arc<HashMap<String, NotifierConfig>>,
) -> Arc<MatchManager<SqliteStateRepository>> {
    let state_repo = SqliteStateRepository::new("sqlite::memory:")
        .await
        .expect("Failed to connect to in-memory db");
    state_repo.run_migrations().await.expect("Failed to run migrations");
    Arc::new(create_test_match_manager_with_repo(notifiers, Arc::new(state_repo)))
}
