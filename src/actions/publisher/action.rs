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
            ActionPayload::Single(ref p) => p.transaction_hash,
            ActionPayload::Aggregated { .. } => Err(ActionDispatcherError::InternalError(
                "Aggregated payloads are not supported for PublisherAction".to_string(),
            ))?,
        };

        let context = payload.context()?;
        let payload_bytes = serde_json::to_vec(&context)?;

        self.publisher
            .publish(&self.topic, &key.to_string(), &payload_bytes)
            .await
            .map_err(ActionDispatcherError::Publisher)?;

        Ok(())
    }

    async fn shutdown(&self) -> Result<(), ActionDispatcherError> {
        self.publisher
            .flush(std::time::Duration::from_secs(5))
            .await
            .map_err(ActionDispatcherError::Publisher)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };

    use alloy::primitives::TxHash;
    use serde_json::json;

    use super::*;
    use crate::{actions::publisher::PublisherError, models::monitor_match::MonitorMatch};

    fn create_monitor_match(monitor_name: &str, action_name: &str) -> MonitorMatch {
        MonitorMatch::new_tx_match(
            1,
            monitor_name.to_string(),
            action_name.to_string(),
            123,
            Default::default(),
            json!({ "key": "value" }),
        )
    }

    #[tokio::test]
    async fn test_publisher_action_execute() {
        let monitor_match = create_monitor_match("Test Monitor", "test_action");
        let payload = ActionPayload::Single(monitor_match);
        const TOPIC_STR: &str = "test_topic";

        struct MockPublisher {
            payload: Vec<u8>,
        }

        #[async_trait::async_trait]
        impl EventPublisher for MockPublisher {
            async fn publish(
                &self,
                topic: &str,
                key: &str,
                payload: &[u8],
            ) -> Result<(), PublisherError> {
                assert_eq!(topic, TOPIC_STR);
                assert_eq!(key, TxHash::default().to_string());
                assert_eq!(payload, self.payload.as_slice());
                Ok(())
            }

            async fn flush(&self, _timeout: Duration) -> Result<(), PublisherError> {
                Ok(())
            }
        }

        let ctx = payload.context().unwrap();
        let action = PublisherAction::new(
            TOPIC_STR.to_string(),
            Box::new(MockPublisher { payload: serde_json::to_vec(&ctx).unwrap() }),
        );

        let result = action.execute(payload).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_publisher_action_execute_aggregated() {
        let monitor_match = create_monitor_match("Test Monitor", "test_action");
        let payload = ActionPayload::Aggregated {
            action_name: "test_action".to_string(),
            matches: vec![monitor_match],
            template: Default::default(),
        };

        struct MockPublisher;

        #[async_trait::async_trait]
        impl EventPublisher for MockPublisher {
            async fn publish(
                &self,
                _topic: &str,
                _key: &str,
                _payload: &[u8],
            ) -> Result<(), PublisherError> {
                Ok(())
            }

            async fn flush(&self, _timeout: Duration) -> Result<(), PublisherError> {
                Ok(())
            }
        }

        let action = PublisherAction::new("test_topic".to_string(), Box::new(MockPublisher));

        let result = action.execute(payload).await;
        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), ActionDispatcherError::InternalError(_)));
    }

    #[tokio::test]
    async fn test_publisher_action_shutdown() {
        struct MockPublisher {
            flushed: Arc<AtomicBool>,
        }

        #[async_trait::async_trait]
        impl EventPublisher for MockPublisher {
            async fn publish(
                &self,
                _topic: &str,
                _key: &str,
                _payload: &[u8],
            ) -> Result<(), PublisherError> {
                Ok(())
            }

            async fn flush(&self, _timeout: Duration) -> Result<(), PublisherError> {
                self.flushed.store(true, Ordering::SeqCst);
                Ok(())
            }
        }

        let flushed_flag = Arc::new(AtomicBool::new(false));
        let publisher = MockPublisher { flushed: Arc::clone(&flushed_flag) };
        let action = PublisherAction::new("test_topic".to_string(), Box::new(publisher));

        let result = action.shutdown().await;
        assert!(result.is_ok());
        assert!(flushed_flag.load(Ordering::SeqCst));
    }
}
