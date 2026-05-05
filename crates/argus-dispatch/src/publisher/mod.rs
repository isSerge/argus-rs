//! Event publishers for different messaging systems (e.g., Kafka, RabbitMQ).

mod error;
#[cfg(feature = "kafka")]
mod kafka;
#[cfg(feature = "nats")]
mod nats;
#[cfg(feature = "rabbitmq")]
mod rabbitmq;
mod traits;

pub use error::PublisherError;
#[cfg(feature = "kafka")]
pub use kafka::KafkaEventPublisher;
#[cfg(feature = "nats")]
pub use nats::NatsEventPublisher;
#[cfg(feature = "rabbitmq")]
pub use rabbitmq::RabbitMqEventPublisher;
pub use traits::EventPublisher;
