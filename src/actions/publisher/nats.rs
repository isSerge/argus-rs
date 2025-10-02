use async_nats::HeaderMap;
use bytes::Bytes;

use crate::{
    actions::{
        ActionPayload,
        error::ActionDispatcherError,
        publisher::{EventPublisher, PublisherError},
        traits::Action,
    },
    models::action::NatsConfig,
};

/// A NATS event publisher.
pub struct NatsEventPublisher {
    /// The NATS client.
    client: async_nats::Client,

    /// The subject to publish messages to.
    subject: String,
}

impl NatsEventPublisher {
    /// Creates a new `NatsEventPublisher` from the given configuration.
    pub async fn from_config(config: &NatsConfig) -> Result<Self, PublisherError> {
        let options = match &config.credentials {
            Some(creds) => match &creds.token {
                Some(token) => async_nats::ConnectOptions::with_token(token.clone()),
                None => match &creds.file {
                    Some(file) => async_nats::ConnectOptions::with_credentials_file(file)
                        .await
                        .map_err(|e| PublisherError::Nats(e.to_string()))?,
                    None => async_nats::ConnectOptions::new(),
                },
            },
            None => async_nats::ConnectOptions::new(),
        };
        let client =
            options.connect(&config.urls).await.map_err(|e| PublisherError::Nats(e.to_string()))?;
        let subject = config.subject.clone();

        Ok(NatsEventPublisher { client, subject })
    }
}

#[async_trait::async_trait]
impl EventPublisher for NatsEventPublisher {
    async fn publish(
        &self,
        subject: &str,
        key: &str,
        payload: &[u8],
    ) -> Result<(), PublisherError> {
        let mut headers = HeaderMap::new();

        if !key.is_empty() {
            headers.insert("X-Message-Key", key);
        }

        self.client
            .publish_with_headers(subject.to_string(), headers, Bytes::copy_from_slice(payload))
            .await
            .map_err(|e| PublisherError::Nats(format!("Failed to publish to NATS: {e}")))?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl Action for NatsEventPublisher {
    async fn execute(&self, payload: ActionPayload) -> Result<(), ActionDispatcherError> {
        match &payload {
            ActionPayload::Single(monitor_match) => {
                let context = payload.context()?;
                let serialized_payload = serde_json::to_vec(&context)?;

                let key = monitor_match.transaction_hash.to_string();

                self.publish(&self.subject, &key, &serialized_payload).await?;

                Ok(())
            }
            ActionPayload::Aggregated { .. } => {
                tracing::warn!("NATS publisher does not support aggregated payloads.");
                Ok(())
            }
        }
    }

    async fn shutdown(&self) -> Result<(), ActionDispatcherError> {
        self.client.drain().await.map_err(|e| {
            ActionDispatcherError::InternalError(format!("Failed to drain NATS client: {e}"))
        })
    }
}
