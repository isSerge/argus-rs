use lapin::{
    Connection, ConnectionProperties, ExchangeKind, options::ExchangeDeclareOptions,
    types::FieldTable,
};

use crate::{
    actions::publisher::{EventPublisher, PublisherError},
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
