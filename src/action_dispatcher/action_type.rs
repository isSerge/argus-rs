use super::{
    Action, ActionDispatcherError, ActionPayload, KafkaEventPublisher, NatsEventPublisher,
    RabbitMqEventPublisher, StdoutAction, WebhookAction,
};

/// An enum representing the different types of actions that can be executed.
pub enum ActionType {
    /// A webhook-based action, which includes generic webhooks as well as
    /// platform-specific ones like Discord, Slack, and Telegram.
    Webhook(WebhookAction),
    /// An action that logs messages to standard output, primarily for testing
    /// and debugging purposes.
    Stdout(StdoutAction),
    /// An action that publishes messages to a Kafka topic.
    Kafka(KafkaEventPublisher),
    /// An action that publishes messages to a RabbitMQ exchange.
    RabbitMq(RabbitMqEventPublisher),
    /// An action that publishes messages to a NATS subject.
    Nats(NatsEventPublisher),
}

/// Macro to avoid writing the same match arms for every method in the trait.
/// Currently used for `execute` and `shutdown` methods
macro_rules! dispatch_action {
    ($self:ident, $method:ident $(, $args:expr)*) => {
        match $self {
            ActionType::Webhook(a) => a.$method($($args),*).await,
            ActionType::Stdout(a) => a.$method($($args),*).await,
            ActionType::Kafka(a) => a.$method($($args),*).await,
            ActionType::RabbitMq(a) => a.$method($($args),*).await,
            ActionType::Nats(a) => a.$method($($args),*).await,
        }
    };
}

/// Implement the Action trait for the Enum itself.
#[async_trait::async_trait]
impl Action for ActionType {
    async fn execute(&self, payload: ActionPayload) -> Result<(), ActionDispatcherError> {
        dispatch_action!(self, execute, payload)
    }

    async fn shutdown(&self) -> Result<(), ActionDispatcherError> {
        dispatch_action!(self, shutdown)
    }
}
