use super::PublisherError;

/// A trait representing an event publisher that can publish events to a
/// messaging system or event bus.
#[async_trait::async_trait]
pub trait EventPublisher: Send + Sync {
    /// Publish an event to the specified topic with the given key and payload.
    async fn publish(&self, topic: &str, key: &str, payload: &[u8]) -> Result<(), PublisherError>;
}
