//! Event publishers for different messaging systems (e.g., Kafka, RabbitMQ).

mod error;
pub mod kafka;
pub mod rabbitmq;
mod traits;

pub use error::PublisherError;
pub use kafka::KafkaEventPublisher;
pub use rabbitmq::RabbitMqEventPublisher;
pub use traits::EventPublisher;
