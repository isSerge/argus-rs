//! Event publishers for different messaging systems (e.g., Kafka, RabbitMQ).

mod error;
mod kafka;
mod nats;
mod rabbitmq;
mod traits;

pub use error::PublisherError;
pub use kafka::KafkaEventPublisher;
pub use nats::NatsEventPublisher;
pub use rabbitmq::RabbitMqEventPublisher;
pub use traits::EventPublisher;
