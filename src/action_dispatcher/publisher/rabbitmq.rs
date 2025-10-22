//! Event publisher for RabbitMQ.

use lapin::{
    Connection, ConnectionProperties, ExchangeKind, options::ExchangeDeclareOptions,
    types::FieldTable,
};

use crate::{
    action_dispatcher::{
        ActionPayload,
        error::ActionDispatcherError,
        publisher::{EventPublisher, PublisherError},
        traits::Action,
    },
    models::action::RabbitMqConfig,
};

/// A RabbitMQ event publisher.
pub struct RabbitMqEventPublisher {
    /// The RabbitMQ connection.
    channel: lapin::Channel,

    /// The exchange to publish messages to.
    exchange: String,

    /// The default routing key to use for messages.
    default_routing_key: Option<String>,
}

impl RabbitMqEventPublisher {
    /// Creates a new `RabbitMqEventPublisher` from the given configuration.
    pub async fn from_config(config: &RabbitMqConfig) -> Result<Self, PublisherError> {
        let connection = Connection::connect(&config.uri, ConnectionProperties::default()).await?;
        let channel = connection.create_channel().await?;
        let exchange_type = match &config.exchange_type {
            et if et.to_lowercase() == "topic" => ExchangeKind::Topic,
            et if et.to_lowercase() == "fanout" => ExchangeKind::Fanout,
            et if et.to_lowercase() == "headers" => ExchangeKind::Headers,
            _ => ExchangeKind::Direct,
        };

        // Declare the exchange
        channel
            .exchange_declare(
                &config.exchange,
                exchange_type,
                ExchangeDeclareOptions { durable: true, ..Default::default() },
                FieldTable::default(),
            )
            .await?;

        Ok(Self {
            channel,
            exchange: config.exchange.clone(),
            default_routing_key: config.routing_key.clone(),
        })
    }
}

#[async_trait::async_trait]
impl EventPublisher for RabbitMqEventPublisher {
    async fn publish(&self, topic: &str, _key: &str, payload: &[u8]) -> Result<(), PublisherError> {
        let routing_key = self.default_routing_key.as_deref().unwrap_or(topic);

        self.channel
            .basic_publish(
                &self.exchange,
                routing_key,
                lapin::options::BasicPublishOptions::default(),
                payload,
                lapin::BasicProperties::default(),
            )
            .await? // Wait for the publish
            .await
            .map(|_| ()) // Wait for the confirmation
            .map_err(PublisherError::from) // Convert lapin::Error to PublisherError
    }
}

#[async_trait::async_trait]
impl Action for RabbitMqEventPublisher {
    async fn execute(&self, payload: ActionPayload) -> Result<(), ActionDispatcherError> {
        match &payload {
            ActionPayload::Single(monitor_match) => {
                let context = payload.context()?;
                let serialized_payload = serde_json::to_vec(&context)
                    .map_err(ActionDispatcherError::DeserializationError)?;

                let key = monitor_match.transaction_hash.to_string();

                self.publish(&self.exchange, &key, &serialized_payload).await?;

                Ok(())
            }
            ActionPayload::Aggregated { .. } => {
                tracing::warn!("RabbitMQ publisher does not support aggregated payloads.");
                Ok(())
            }
        }
    }

    async fn shutdown(&self) -> Result<(), ActionDispatcherError> {
        self.channel.close(200, "Goodbye").await.map_err(|e| {
            ActionDispatcherError::InternalError(format!("Failed to close RabbitMQ channel: {e}"))
        })
    }
}
