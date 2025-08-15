//! Webhook notification implementation.
//!
//! Provides functionality to send formatted messages to webhooks
//! via incoming webhooks, supporting message templates with variable
//! substitution.

use std::{collections::HashMap, sync::Arc};

use chrono::Utc;
use hmac::{Hmac, Mac};
use reqwest::{
    Method,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use reqwest_middleware::ClientWithMiddleware;
use sha2::Sha256;

use super::error::NotificationError;

/// HMAC SHA256 type alias
type HmacSha256 = Hmac<Sha256>;

/// Represents a webhook configuration
#[derive(Clone)]
pub struct WebhookConfig {
    pub url: String,
    pub url_params: Option<HashMap<String, String>>,
    pub title: String,
    pub body_template: String,
    pub method: Option<String>,
    pub secret: Option<String>,
    pub headers: Option<HashMap<String, String>>,
}

/// Implementation of webhook notifications via webhooks
#[derive(Debug)]
pub struct WebhookNotifier {
    /// Webhook URL for message delivery
    pub url: String,
    /// URL parameters to use for the webhook request
    pub url_params: Option<HashMap<String, String>>,
    /// Configured HTTP client for webhook requests with retry capabilities
    pub client: Arc<ClientWithMiddleware>,
    /// HTTP method to use for the webhook request
    pub method: Option<String>,
    /// Secret to use for the webhook request
    pub secret: Option<String>,
    /// Headers to use for the webhook request
    pub headers: Option<HashMap<String, String>>,
}

impl WebhookNotifier {
    /// Creates a new Webhook notifier instance
    ///
    /// # Arguments
    /// * `config` - Webhook configuration
    /// * `http_client` - HTTP client with middleware for retries
    ///
    /// # Returns
    /// * `Result<Self, NotificationError>` - Notifier instance if config is
    ///   valid
    pub fn new(
        config: WebhookConfig,
        http_client: Arc<ClientWithMiddleware>,
    ) -> Result<Self, NotificationError> {
        let mut headers = config.headers.unwrap_or_default();
        if !headers.contains_key("Content-Type") {
            headers.insert("Content-Type".to_string(), "application/json".to_string());
        }
        Ok(Self {
            url: config.url,
            url_params: config.url_params,
            client: http_client,
            method: Some(config.method.unwrap_or("POST".to_string())),
            secret: config.secret,
            headers: Some(headers),
        })
    }

    pub fn sign_payload(
        &self,
        secret: &str,
        payload: &serde_json::Value,
    ) -> Result<(String, String), NotificationError> {
        // Explicitly reject empty secret, because `HmacSha256::new_from_slice`
        // currently allows empty secrets
        if secret.is_empty() {
            return Err(NotificationError::NotifyFailed(
                "Invalid secret: cannot be empty.".to_string(),
            ));
        }

        let timestamp = Utc::now().timestamp_millis();

        // Create HMAC instance
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
            .map_err(|e| NotificationError::ConfigError(format!("Invalid secret: {e}")))?;

        // Create the message to sign
        let serialized_payload = serde_json::to_string(payload).map_err(|e| {
            NotificationError::InternalError(format!("Failed to serialize payload: {e}"))
        })?;
        let message = format!("{serialized_payload}{timestamp}");
        mac.update(message.as_bytes());

        // Get the HMAC result
        let signature = hex::encode(mac.finalize().into_bytes());

        Ok((signature, timestamp.to_string()))
    }

    /// Sends a JSON payload to Webhook
    ///
    /// # Arguments
    /// * `payload` - The JSON payload to send
    ///
    /// # Returns
    /// * `Result<(), NotificationError>` - Success or error
    pub async fn notify_json(&self, payload: &serde_json::Value) -> Result<(), NotificationError> {
        let mut url = self.url.clone();
        // Add URL parameters if present
        if let Some(params) = &self.url_params {
            let params_str: Vec<String> =
                params.iter().map(|(k, v)| format!("{}={}", k, urlencoding::encode(v))).collect();
            if !params_str.is_empty() {
                url = format!("{}?{}", url, params_str.join("&"));
            }
        }

        let method = if let Some(ref m) = self.method {
            Method::from_bytes(m.as_bytes()).unwrap_or(Method::POST)
        } else {
            Method::POST
        };

        // Add default headers
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("content-type"),
            HeaderValue::from_static("application/json"),
        );

        if let Some(secret) = &self.secret {
            let (signature, timestamp) = self.sign_payload(secret, payload)?;

            // Add signature headers
            headers.insert(
                HeaderName::from_static("x-signature"),
                HeaderValue::from_str(&signature).map_err(|e| {
                    NotificationError::NotifyFailed(format!("Invalid signature value: {e}"))
                })?,
            );
            headers.insert(
                HeaderName::from_static("x-timestamp"),
                HeaderValue::from_str(&timestamp).map_err(|e| {
                    NotificationError::NotifyFailed(format!("Invalid timestamp value: {e}"))
                })?,
            );
        }

        // Add custom headers
        if let Some(headers_map) = &self.headers {
            for (key, value) in headers_map {
                let header_name = HeaderName::from_bytes(key.as_bytes()).map_err(|e| {
                    NotificationError::NotifyFailed(format!("Invalid header name: {key}: {e}"))
                })?;
                let header_value = HeaderValue::from_str(value).map_err(|e| {
                    NotificationError::NotifyFailed(format!(
                        "Invalid header value for {key}: {value}: {e}"
                    ))
                })?;
                headers.insert(header_name, header_value);
            }
        }

        // Send request with custom payload
        let response =
            self.client.request(method, url.as_str()).headers(headers).json(payload).send().await?;

        let status = response.status();

        if !status.is_success() {
            return Err(NotificationError::NotifyFailed(format!(
                "Webhook request failed with status: {status}"
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mockito::{Matcher, Mock};
    use serde_json::json;

    use super::*;
    use crate::notification::payload_builder::{
        GenericWebhookPayloadBuilder, WebhookPayloadBuilder,
    };

    fn create_test_http_client() -> Arc<ClientWithMiddleware> {
        Arc::new(reqwest_middleware::ClientBuilder::new(reqwest::Client::new()).build())
    }

    fn create_test_notifier(
        url: &str,
        secret: Option<&str>,
        headers: Option<HashMap<String, String>>,
    ) -> WebhookNotifier {
        let http_client = create_test_http_client();
        let config = WebhookConfig {
            url: url.to_string(),
            url_params: None,
            title: "Alert".to_string(),
            body_template: "Test message".to_string(),
            method: Some("POST".to_string()),
            secret: secret.map(|s| s.to_string()),
            headers,
        };
        WebhookNotifier::new(config, http_client).unwrap()
    }

    fn create_test_payload() -> serde_json::Value {
        GenericWebhookPayloadBuilder.build_payload("Test Alert", "Test message with value ${value}")
    }

    ////////////////////////////////////////////////////////////
    // sign_request tests
    ////////////////////////////////////////////////////////////

    #[test]
    fn test_sign_request() {
        let notifier =
            create_test_notifier("https://webhook.example.com", Some("test-secret"), None);
        let payload = json!({
            "title": "Test Title",
            "body": "Test message"
        });
        let secret = "test-secret";

        let result = notifier.sign_payload(secret, &payload).unwrap();
        let (signature, timestamp) = result;

        assert!(!signature.is_empty());
        assert!(!timestamp.is_empty());
    }

    #[test]
    fn test_sign_request_fails_empty_secret() {
        let notifier = create_test_notifier("https://webhook.example.com", None, None);
        let payload = json!({
            "title": "Test Title",
            "body": "Test message"
        });
        let empty_secret = "";

        let result = notifier.sign_payload(empty_secret, &payload);
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(matches!(error, NotificationError::NotifyFailed(_)));
    }

    ////////////////////////////////////////////////////////////
    // notify tests
    ////////////////////////////////////////////////////////////

    #[tokio::test]
    async fn test_notify_failure() {
        let notifier = create_test_notifier("https://webhook.example.com", None, None);
        let payload = create_test_payload();
        let result = notifier.notify_json(&payload).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_notify_includes_signature_and_timestamp() {
        let mut server = mockito::Server::new_async().await;
        let mock: Mock = server
            .mock("POST", "/")
            .match_header("X-Signature", Matcher::Regex("^[0-9a-f]{64}$".to_string()))
            .match_header("X-Timestamp", Matcher::Regex("^[0-9]+$".to_string()))
            .match_header("Content-Type", "application/json")
            .with_status(200)
            .create_async()
            .await;

        let notifier = create_test_notifier(
            server.url().as_str(),
            Some("top-secret"),
            Some(HashMap::from([("Content-Type".to_string(), "application/json".to_string())])),
        );

        let payload = create_test_payload();
        let result = notifier.notify_json(&payload).await;

        assert!(result.is_ok());

        mock.assert();
    }

    ////////////////////////////////////////////////////////////
    // notify header validation tests
    ////////////////////////////////////////////////////////////

    #[tokio::test]
    async fn test_notify_with_invalid_header_name() {
        let server = mockito::Server::new_async().await;
        let invalid_headers =
            HashMap::from([("Invalid Header!@#".to_string(), "value".to_string())]);

        let notifier = create_test_notifier(server.url().as_str(), None, Some(invalid_headers));
        let payload = create_test_payload();
        let result = notifier.notify_json(&payload).await;
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Invalid header name"));
    }

    #[tokio::test]
    async fn test_notify_with_invalid_header_value() {
        let server = mockito::Server::new_async().await;
        let invalid_headers =
            HashMap::from([("X-Custom-Header".to_string(), "Invalid\nValue".to_string())]);

        let notifier = create_test_notifier(server.url().as_str(), None, Some(invalid_headers));

        let payload = create_test_payload();
        let result = notifier.notify_json(&payload).await;
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Invalid header value"));
    }

    #[tokio::test]
    async fn test_notify_with_valid_headers() {
        let mut server = mockito::Server::new_async().await;
        let valid_headers = HashMap::from([
            ("X-Custom-Header".to_string(), "valid-value".to_string()),
            ("Accept".to_string(), "application/json".to_string()),
        ]);

        let mock = server
            .mock("POST", "/")
            .match_header("X-Custom-Header", "valid-value")
            .match_header("Accept", "application/json")
            .with_status(200)
            .create_async()
            .await;

        let notifier = create_test_notifier(server.url().as_str(), None, Some(valid_headers));

        let payload = create_test_payload();
        let result = notifier.notify_json(&payload).await;
        assert!(result.is_ok());
        mock.assert();
    }

    #[tokio::test]
    async fn test_notify_signature_header_cases() {
        let mut server = mockito::Server::new_async().await;

        let mock = server
            .mock("POST", "/")
            .match_header("X-Signature", Matcher::Any)
            .match_header("X-Timestamp", Matcher::Any)
            .with_status(200)
            .create_async()
            .await;

        let notifier = create_test_notifier(server.url().as_str(), Some("test-secret"), None);

        let payload = create_test_payload();
        let result = notifier.notify_json(&payload).await;
        assert!(result.is_ok());
        mock.assert();
    }

    #[test]
    fn test_sign_request_validation() {
        let notifier =
            create_test_notifier("https://webhook.example.com", Some("test-secret"), None);

        let payload = create_test_payload();

        let result = notifier.sign_payload("test-secret", &payload).unwrap();
        let (signature, timestamp) = result;

        // Validate signature format (should be a hex string)
        assert!(hex::decode(&signature).is_ok(), "Signature should be valid hex");

        // Validate timestamp format (should be a valid i64)
        assert!(timestamp.parse::<i64>().is_ok(), "Timestamp should be valid i64");
    }
}
