//! # Action Dispatcher
//!
//! The Action Dispatcher is responsible for managing and executing various
//! types of actions based on pre-loaded configurations. It supports actions
//! such as webhook notifications, standard output logging, and message
//! publishing to systems like Kafka, RabbitMQ, etc.

use std::{collections::HashMap, sync::Arc};

use argus_core::models::{
    action::{ActionConfig, ActionTypeConfig},
    monitor_match::MonitorMatch,
};

use crate::http_client::HttpClientPool;

mod action_type;
pub mod error;
pub mod http_client;
pub mod publisher;
mod stdout;
pub mod template;
mod traits;
mod webhook;

// ActionPayload is now defined in argus-core:
use action_type::ActionType;
pub use argus_core::action_dispatcher::ActionPayload;
use error::ActionDispatcherError;
use publisher::{KafkaEventPublisher, NatsEventPublisher, RabbitMqEventPublisher};
use stdout::StdoutAction;
use template::TemplateService;
use tokio::sync::mpsc;
use traits::Action;
use webhook::{WebhookAction, WebhookComponents};

trait IntoWebhookComponents {
    fn as_webhook_components(&self) -> Result<WebhookComponents, ActionDispatcherError>;
}

impl IntoWebhookComponents for ActionTypeConfig {
    /// Transforms the specific action configuration into a generic set of
    /// webhook components.
    fn as_webhook_components(&self) -> Result<WebhookComponents, ActionDispatcherError> {
        Ok(match self {
            ActionTypeConfig::Webhook(c) => c.into(),
            ActionTypeConfig::Discord(c) => c.into(),
            ActionTypeConfig::Telegram(c) => c.into(),
            ActionTypeConfig::Slack(c) => c.into(),
            _ =>
                return Err(ActionDispatcherError::ConfigError(format!(
                    "{:?} action does not support webhook components",
                    self
                ))),
        })
    }
}
/// A service responsible for dispatching actions based on pre-loaded
/// action configurations (webhook notifiers, publishers, etc.)
pub struct ActionDispatcher {
    /// A map of action names to their corresponding action implementations.
    actions: HashMap<String, ActionType>,
}

impl ActionDispatcher {
    /// Creates a new `ActionDispatcher` instance.
    ///
    /// # Arguments
    ///
    /// * `actions` - A vector of `ActionConfig` loaded and validated at
    ///   application startup.
    /// * `client_pool` - A shared pool of HTTP clients.
    pub async fn new(
        action_configs: Arc<HashMap<String, ActionConfig>>,
        client_pool: Arc<HttpClientPool>,
    ) -> Result<Self, ActionDispatcherError> {
        let template_service = Arc::new(TemplateService::new());
        let mut actions: HashMap<String, ActionType> = HashMap::new();

        for (name, config) in action_configs.iter() {
            let action: ActionType = match &config.config {
                // Kafka publisher action
                ActionTypeConfig::Kafka(c) => {
                    let publisher = match KafkaEventPublisher::from_config(c) {
                        Ok(p) => p,
                        Err(e) => {
                            tracing::error!(
                                action_name = name,
                                error = ?e,
                                "Failed to create Kafka publisher"
                            );
                            continue;
                        }
                    };

                    ActionType::Kafka(publisher)
                }

                // RabbitMQ publisher action
                ActionTypeConfig::RabbitMq(c) => {
                    let publisher = match RabbitMqEventPublisher::from_config(c).await {
                        Ok(p) => p,
                        Err(e) => {
                            tracing::error!(
                                action_name = name,
                                error = ?e,
                                "Failed to create RabbitMQ publisher"
                            );
                            continue;
                        }
                    };

                    ActionType::RabbitMq(publisher)
                }

                // NATS publisher action
                ActionTypeConfig::Nats(c) => {
                    let publisher = match NatsEventPublisher::from_config(c).await {
                        Ok(p) => p,
                        Err(e) => {
                            tracing::error!(
                                action_name = name,
                                error = ?e,
                                "Failed to create NATS publisher"
                            );
                            continue;
                        }
                    };

                    ActionType::Nats(publisher)
                }

                // Standard output action
                ActionTypeConfig::Stdout(c) =>
                    ActionType::Stdout(StdoutAction::new(c.clone(), template_service.clone())),

                // All webhook-based actions are constructed here
                ActionTypeConfig::Webhook(_)
                | ActionTypeConfig::Discord(_)
                | ActionTypeConfig::Slack(_)
                | ActionTypeConfig::Telegram(_) => {
                    // This unwrap is safe because we've already filtered non-webhook types
                    let components = config.config.as_webhook_components().unwrap();
                    let http_client = client_pool.get_or_create(&components.retry_policy).await?;
                    ActionType::Webhook(WebhookAction::new(
                        components,
                        http_client,
                        template_service.clone(),
                    ))
                }
            };
            actions.insert(name.clone(), action);
        }

        Ok(ActionDispatcher { actions })
    }

    /// Executes a notification for a given action.
    pub async fn execute(&self, payload: ActionPayload) -> Result<(), ActionDispatcherError> {
        let action_name = payload.action_name();

        tracing::debug!(action = %action_name, "Executing action.");

        let action = &self.actions.get(&action_name).ok_or_else(|| {
            ActionDispatcherError::ConfigError(format!("Action '{}' not found", action_name))
        })?;

        action.execute(payload).await
    }

    /// Runs the notification service, listening for incoming monitor matches
    /// and executing notifications based on the configured actions.
    pub async fn run(&self, mut notifications_rx: mpsc::Receiver<MonitorMatch>) {
        while let Some(monitor_match) = notifications_rx.recv().await {
            if let Err(e) = self.execute(ActionPayload::Single(monitor_match.clone())).await {
                tracing::error!(
                    "Failed to execute notification for action '{}': {}",
                    monitor_match.action_name,
                    e
                );
            }
        }
    }

    /// Shuts down all the actions managed by the dispatcher.
    pub async fn shutdown(&self) {
        tracing::info!("Shutting down all actions...");
        let shutdowns = self.actions.values().map(|action| action.shutdown());
        let results = futures::future::join_all(shutdowns).await;

        for result in results {
            if let Err(e) = result {
                tracing::error!("Error shutting down action: {}", e);
            }
        }
    }
}
