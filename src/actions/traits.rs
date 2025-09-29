use crate::actions::ActionPayload;

#[async_trait::async_trait]
pub trait Action: Send + Sync {
    /// Executes the action with the given notification payload.
    async fn execute(&self, payload: ActionPayload) -> Result<(), String>;
}
