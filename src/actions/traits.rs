use crate::actions::{ActionPayload, error::ActionDispatcherError};

/// A trait representing an action that can be executed in response to a monitor
/// match.
#[async_trait::async_trait]
pub trait Action: Send + Sync {
    /// Executes the action with the given notification payload.
    async fn execute(&self, payload: ActionPayload) -> Result<(), ActionDispatcherError>;

    /// Shuts down the action, performing any necessary cleanup.
    async fn shutdown(&self) -> Result<(), ActionDispatcherError> {
        Ok(())
    }
}
