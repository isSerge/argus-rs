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
#[cfg(feature = "kafka")]
use publisher::KafkaEventPublisher;
#[cfg(feature = "nats")]
use publisher::NatsEventPublisher;
#[cfg(feature = "rabbitmq")]
use publisher::RabbitMqEventPublisher;
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
                #[cfg(feature = "kafka")]
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
                #[cfg(feature = "rabbitmq")]
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
                #[cfg(feature = "nats")]
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

                #[allow(unreachable_patterns)]
                _ => {
                    return Err(ActionDispatcherError::ConfigError(format!(
                        "Action '{}' uses a type that is not compiled into this build (feature \
                         flag disabled). Either enable the required feature flag or remove this \
                         action from the configuration.",
                        name
                    )));
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

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use alloy::primitives::{TxHash, address};
    use argus_core::{
        config::HttpRetryConfig,
        models::{
            action::{
                ActionTypeConfig, DiscordConfig, GenericWebhookConfig, SlackConfig, StdoutConfig,
                TelegramConfig,
            },
            monitor_match::LogDetails,
            notification::NotificationMessage,
        },
        test_utils::ActionBuilder,
    };
    use serde_json::json;
    use url::Url;

    use super::*;

    fn create_mock_monitor_match(action_name: &str) -> MonitorMatch {
        let log_details = LogDetails {
            address: address!("1234567890abcdef1234567890abcdef12345678"),
            log_index: 15,
            name: "TestLog".to_string(),
            params: json!({"param1": "value1", "param2": 42}),
        };
        MonitorMatch::builder(
            1,
            "test monitor".to_string(),
            action_name.to_string(),
            123,
            TxHash::default(),
        )
        .log_match(log_details, json!({}))
        .decoded_call(None)
        .build()
    }

    #[tokio::test]
    async fn test_missing_action_error() {
        let service =
            ActionDispatcher::new(Arc::new(HashMap::new()), Arc::new(HttpClientPool::default()))
                .await
                .unwrap();

        let result =
            service.execute(ActionPayload::Single(create_mock_monitor_match("nonexistent"))).await;

        assert!(matches!(
            result,
            Err(ActionDispatcherError::ConfigError(ref msg)) if msg.contains("Action 'nonexistent' not found")
        ));
    }

    #[test]
    fn as_webhook_components_slack() {
        let url = Url::parse("https://slack.example.com").unwrap();
        let config = ActionTypeConfig::Slack(SlackConfig {
            slack_url: url.clone(),
            message: NotificationMessage { title: "T".into(), body: "B".into() },
            retry_policy: HttpRetryConfig::default(),
        });

        let components = config.as_webhook_components().unwrap();

        assert_eq!(components.config.url, url);
        assert_eq!(components.config.title, "T");
        assert_eq!(components.config.body_template, "B");
        assert_eq!(components.config.method, Some("POST".to_string()));
        assert!(components.config.secret.is_none());
        let payload = components.builder.build_payload("T", "B");
        assert!(payload.get("blocks").is_some(), "expected Slack 'blocks' payload");
        assert!(payload.get("content").is_none());
    }

    #[test]
    fn as_webhook_components_discord() {
        let url = Url::parse("https://discord.example.com").unwrap();
        let config = ActionTypeConfig::Discord(DiscordConfig {
            discord_url: url.clone(),
            message: NotificationMessage { title: "T".into(), body: "B".into() },
            retry_policy: HttpRetryConfig::default(),
        });

        let components = config.as_webhook_components().unwrap();

        assert_eq!(components.config.url, url);
        assert_eq!(components.config.method, Some("POST".to_string()));
        assert!(components.config.secret.is_none());
        let payload = components.builder.build_payload("T", "B");
        assert_eq!(payload.get("content").unwrap(), "*T*\n\nB");
        assert!(payload.get("blocks").is_none());
    }

    #[test]
    fn as_webhook_components_telegram() {
        let config = ActionTypeConfig::Telegram(TelegramConfig {
            token: "mytoken123".into(),
            chat_id: "cid".into(),
            message: NotificationMessage { title: "T".into(), body: "B".into() },
            disable_web_preview: Some(true),
            retry_policy: HttpRetryConfig::default(),
        });

        let components = config.as_webhook_components().unwrap();

        assert_eq!(
            components.config.url,
            Url::parse("https://api.telegram.org/botmytoken123/sendMessage").unwrap()
        );
        let payload = components.builder.build_payload("T", "B");
        assert_eq!(payload.get("chat_id").unwrap(), "cid");
        assert_eq!(payload.get("text").unwrap(), "*T* \n\nB");
        assert_eq!(payload.get("disable_web_page_preview").unwrap(), &json!(true));
    }

    #[test]
    fn as_webhook_components_generic_webhook() {
        let url = Url::parse("https://webhook.example.com").unwrap();
        let mut headers = HashMap::new();
        headers.insert("X-Test".to_string(), "val".to_string());
        let config = ActionTypeConfig::Webhook(GenericWebhookConfig {
            url: url.clone(),
            message: NotificationMessage { title: "T".into(), body: "B".into() },
            method: Some("PUT".to_string()),
            secret: Some("s3cr3t".to_string()),
            headers: Some(headers.clone()),
            retry_policy: HttpRetryConfig::default(),
        });

        let components = config.as_webhook_components().unwrap();

        assert_eq!(components.config.url, url);
        assert_eq!(components.config.method, Some("PUT".to_string()));
        assert_eq!(components.config.secret, Some("s3cr3t".to_string()));
        assert_eq!(components.config.headers, Some(headers));
        let payload = components.builder.build_payload("T", "B");
        assert_eq!(payload.get("title").unwrap(), "T");
        assert_eq!(payload.get("body").unwrap(), "B");
    }

    #[test]
    fn as_webhook_components_fails_for_stdout() {
        let config = ActionTypeConfig::Stdout(StdoutConfig { message: None });
        let result = config.as_webhook_components();
        assert!(matches!(
            result,
            Err(ActionDispatcherError::ConfigError(ref msg)) if msg.contains("action does not support webhook components")
        ));
    }

    #[tokio::test]
    async fn test_shutdown_no_panic() {
        let action_config = ActionBuilder::new("stdout_test").stdout_config(None).build();
        let configs = Arc::new(
            [(action_config.name.clone(), action_config)].into_iter().collect::<HashMap<_, _>>(),
        );
        let service =
            ActionDispatcher::new(configs, Arc::new(HttpClientPool::default())).await.unwrap();

        service.shutdown().await; // must not panic
    }

    /// When all publisher features are disabled the catch-all arm returns an
    /// error instead of silently skipping the unsupported action type.
    ///
    /// This test is only meaningful when none of kafka/rabbitmq/nats is
    /// compiled in, so it is gated accordingly.
    #[cfg(not(any(feature = "kafka", feature = "rabbitmq", feature = "nats")))]
    #[tokio::test]
    async fn test_unsupported_action_type_returns_error() {
        let action_config =
            ActionBuilder::new("kafka_action").kafka_config("localhost:9092", "test-topic").build();
        let configs = Arc::new(
            [(action_config.name.clone(), action_config)].into_iter().collect::<HashMap<_, _>>(),
        );

        let result = ActionDispatcher::new(configs, Arc::new(HttpClientPool::default())).await;

        assert!(matches!(
            result,
            Err(ActionDispatcherError::ConfigError(ref msg))
                if msg.contains("kafka_action") && msg.contains("feature flag disabled")
        ));
    }
}
