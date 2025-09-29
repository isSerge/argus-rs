use crate::actions::{
    ActionPayload, error::ActionDispatcherError, publisher::EventPublisher, traits::Action,
};

pub struct PublisherAction {
    topic: String,
    publisher: Box<dyn EventPublisher>,
}

impl PublisherAction {
    pub fn new(topic: String, publisher: Box<dyn EventPublisher>) -> Self {
        Self { topic, publisher }
    }
}

#[async_trait::async_trait]
impl Action for PublisherAction {
    async fn execute(&self, payload: ActionPayload) -> Result<(), ActionDispatcherError> {
        let key = match payload {
            ActionPayload::Single(ref p) => p.transaction_hash.clone(),
            ActionPayload::Aggregated { .. } => Err(ActionDispatcherError::InternalError(
                "Aggregated payloads are not supported for PublisherAction".to_string(),
            ))?,
        };

        let context = payload.context()?;
        let payload_bytes = serde_json::to_vec(&context)?;

        self.publisher
            .publish(&self.topic, &key.to_string(), &payload_bytes)
            .await
            .map_err(|e| ActionDispatcherError::Publisher(e))?;

        Ok(())
    }
}
