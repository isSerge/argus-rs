//! Loads and validates trigger configurations from a YAML file.

use crate::{
    config::HttpRetryConfig, models::notification::NotificationMessage,
    notification::error::NotificationError,
};
use config::{Config, File};
use serde::Deserialize;
use std::{collections::HashMap, fs, path::PathBuf};
use thiserror::Error;
use url::Url;

/// Configuration for a generic webhook.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct WebhookConfig {
    /// The URL of the webhook endpoint.
    pub url: String,
    /// The HTTP method to use for the webhook (e.g., "POST", "GET").
    pub method: Option<String>,
    /// An optional secret for signing webhook requests.
    pub secret: Option<String>,
    /// Optional custom headers to include in the webhook request.
    pub headers: Option<HashMap<String, String>>,
    /// The message content for the notification.
    pub message: NotificationMessage,
    /// The retry policy configuration for HTTP requests.
    #[serde(default)]
    pub retry_policy: HttpRetryConfig,
}

/// Configuration for a Slack notification.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct SlackConfig {
    /// The Slack webhook URL.
    pub slack_url: String,
    /// The message content for the notification.
    pub message: NotificationMessage,
    /// The retry policy configuration for HTTP requests.
    #[serde(default)]
    pub retry_policy: HttpRetryConfig,
}

/// Configuration for a Discord notification.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct DiscordConfig {
    /// The Discord webhook URL.
    pub discord_url: String,
    /// The message content for the notification.
    pub message: NotificationMessage,
    /// The retry policy configuration for HTTP requests.
    #[serde(default)]
    pub retry_policy: HttpRetryConfig,
}

/// Configuration for a Telegram notification.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TelegramConfig {
    /// The Telegram bot token.
    pub token: String,
    /// The chat ID to send the message to.
    pub chat_id: String,
    /// The message content for the notification.
    pub message: NotificationMessage,
    /// Whether to disable web page preview for the message.
    pub disable_web_preview: Option<bool>,
    /// The retry policy configuration for HTTP requests.
    #[serde(default)]
    pub retry_policy: HttpRetryConfig,
}

/// An enum representing the different types of trigger configurations.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TriggerTypeConfig {
    /// A generic webhook trigger.
    Webhook(WebhookConfig),
    /// A Slack notification trigger.
    Slack(SlackConfig),
    /// A Discord notification trigger.
    Discord(DiscordConfig),
    /// A Telegram notification trigger.
    Telegram(TelegramConfig),
}

impl TriggerTypeConfig {
    /// Validates the trigger configuration.
    pub fn validate(&self) -> Result<(), NotificationError> {
        match self {
            TriggerTypeConfig::Webhook(config) => {
                if Url::parse(&config.url).is_err() {
                    return Err(NotificationError::ConfigError(format!(
                        "Invalid webhook URL: {}",
                        config.url
                    )));
                }
                if config.message.title.is_empty() {
                    return Err(NotificationError::ConfigError(
                        "Webhook title cannot be empty.".to_string(),
                    ));
                }
                Ok(())
            }
            TriggerTypeConfig::Slack(config) => {
                if Url::parse(&config.slack_url).is_err() {
                    return Err(NotificationError::ConfigError(format!(
                        "Invalid Slack URL: {}",
                        config.slack_url
                    )));
                }
                if !config.slack_url.starts_with("https://hooks.slack.com/") {
                    return Err(NotificationError::ConfigError(
                        "Slack URL appears to be invalid.".to_string(),
                    ));
                }
                Ok(())
            }
            TriggerTypeConfig::Discord(config) => {
                if Url::parse(&config.discord_url).is_err() {
                    return Err(NotificationError::ConfigError(format!(
                        "Invalid Discord URL: {}",
                        config.discord_url
                    )));
                }
                Ok(())
            }
            TriggerTypeConfig::Telegram(config) => {
                if config.token.is_empty() {
                    return Err(NotificationError::ConfigError(
                        "Telegram token cannot be empty.".to_string(),
                    ));
                }
                if config.chat_id.is_empty() {
                    return Err(NotificationError::ConfigError(
                        "Telegram chat_id cannot be empty.".to_string(),
                    ));
                }
                Ok(())
            }
        }
    }
}

/// Represents a single trigger configuration from the YAML file.
#[derive(Debug, Clone, Deserialize)]
pub struct TriggerConfig {
    /// The unique name of the trigger.
    pub name: String,
    /// The specific configuration for the trigger type.
    #[serde(flatten)]
    pub config: TriggerTypeConfig,
}

/// Container for trigger configurations loaded from a file.
#[derive(Debug, Clone, Deserialize)]
pub struct TriggerConfigFile {
    /// A list of trigger configurations.
    pub triggers: Vec<TriggerConfig>,
}

/// Loads trigger configurations from a file.
pub struct TriggerLoader {
    path: PathBuf,
}

/// Errors that can occur while loading trigger configurations.
#[derive(Debug, Error)]
pub enum TriggerLoaderError {
    /// An I/O error occurred while reading the trigger configuration file.
    #[error("Failed to load trigger configuration: {0}")]
    IoError(#[from] std::io::Error),

    /// An error occurred while parsing the trigger configuration file.
    #[error("Failed to parse trigger configuration: {0}")]
    ParseError(String),

    /// The trigger configuration file has an unsupported format (e.g., not YAML).
    #[error("Unsupported trigger configuration format")]
    UnsupportedFormat,

    /// The trigger configuration is invalid (e.g., missing required fields, invalid URLs).
    #[error("Invalid trigger configuration: {0}")]
    ValidationError(#[from] NotificationError),
}

impl TriggerLoader {
    /// Creates a new `TriggerLoader` instance.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Loads and validates the trigger configurations from the specified file.
    pub fn load(&self) -> Result<Vec<TriggerConfig>, TriggerLoaderError> {
        if !self.is_yaml_file() {
            return Err(TriggerLoaderError::UnsupportedFormat);
        }

        let config_str = fs::read_to_string(&self.path)?;
        let config: TriggerConfigFile = Config::builder()
            .add_source(File::from_str(&config_str, config::FileFormat::Yaml))
            .build()
            .map_err(|e| TriggerLoaderError::ParseError(e.to_string()))?
            .try_deserialize()
            .map_err(|e| TriggerLoaderError::ParseError(e.to_string()))?;

        for trigger_config in &config.triggers {
            trigger_config.config.validate()?;
        }

        Ok(config.triggers)
    }

    /// Checks if the file has a YAML extension.
    fn is_yaml_file(&self) -> bool {
        matches!(
            self.path.extension().and_then(|ext| ext.to_str()),
            Some("yaml") | Some("yml")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::notification::NotificationMessage;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_dir_with_file(yaml_filename: &str, yaml_content: &str) -> (TempDir, PathBuf) {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let yaml_path = temp_dir.path().join(yaml_filename);
        fs::write(&yaml_path, yaml_content).expect("Failed to write YAML file");
        (temp_dir, yaml_path)
    }

    #[test]
    fn test_load_valid_webhook_trigger() {
        let yaml_content = r###"
triggers:
  - name: "test_webhook"
    webhook:
      url: "http://example.com/webhook"
      method: "POST"
      secret: "mysecret"
      headers:
        Content-Type: "application/json"
      message:
        title: "Test Title"
        body: "Test Body"
        footer: "Test Footer"
        color: "#FF0000"
        fields:
          - name: "Field1"
            value: "Value1"
          - name: "Field2"
            value: "Value2"
"###;
        let (_temp_dir, file_path) = create_test_dir_with_file("triggers.yaml", yaml_content);
        let loader = TriggerLoader::new(file_path);
        let triggers = loader.load().unwrap();

        assert_eq!(triggers.len(), 1);
        let trigger = &triggers[0];
        assert_eq!(trigger.name, "test_webhook");
        if let TriggerTypeConfig::Webhook(config) = &trigger.config {
            assert_eq!(config.url, "http://example.com/webhook");
            assert_eq!(config.method, Some("POST".to_string()));
            assert_eq!(config.secret, Some("mysecret".to_string()));
            assert!(config.headers.is_some());
            assert_eq!(config.message.title, "Test Title");
        } else {
            panic!("Expected WebhookConfig");
        }
    }

    #[test]
    fn test_load_valid_slack_trigger() {
        let yaml_content = r#"
triggers:
  - name: "test_slack"
    slack:
      slack_url: "https://hooks.slack.com/services/T00000000/B00000000/XXXXXXXXXXXXXXXXXXXXXXXX"
      message:
        title: "Slack Title"
        body: "Slack Body"
"#;
        let (_temp_dir, file_path) = create_test_dir_with_file("triggers.yaml", yaml_content);
        let loader = TriggerLoader::new(file_path);
        let triggers = loader.load().unwrap();

        assert_eq!(triggers.len(), 1);
        let trigger = &triggers[0];
        assert_eq!(trigger.name, "test_slack");
        if let TriggerTypeConfig::Slack(config) = &trigger.config {
            assert_eq!(
                config.slack_url,
                "https://hooks.slack.com/services/T00000000/B00000000/XXXXXXXXXXXXXXXXXXXXXXXX"
            );
            assert_eq!(config.message.title, "Slack Title");
        } else {
            panic!("Expected SlackConfig");
        }
    }

    #[test]
    fn test_load_valid_discord_trigger() {
        let yaml_content = r#"
triggers:
  - name: "test_discord"
    discord:
      discord_url: "https://discord.com/api/webhooks/12345/abcdef"
      message:
        title: "Discord Title"
        body: "Discord Body"
"#;
        let (_temp_dir, file_path) = create_test_dir_with_file("triggers.yaml", yaml_content);
        let loader = TriggerLoader::new(file_path);
        let triggers = loader.load().unwrap();

        assert_eq!(triggers.len(), 1);
        let trigger = &triggers[0];
        assert_eq!(trigger.name, "test_discord");
        if let TriggerTypeConfig::Discord(config) = &trigger.config {
            assert_eq!(
                config.discord_url,
                "https://discord.com/api/webhooks/12345/abcdef"
            );
            assert_eq!(config.message.title, "Discord Title");
        } else {
            panic!("Expected DiscordConfig");
        }
    }

    #[test]
    fn test_load_valid_telegram_trigger() {
        let yaml_content = r#"
triggers:
  - name: "test_telegram"
    telegram:
      token: "123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11"
      chat_id: "-1234567890"
      message:
        title: "Telegram Title"
        body: "Telegram Body"
"#;
        let (_temp_dir, file_path) = create_test_dir_with_file("triggers.yaml", yaml_content);
        let loader = TriggerLoader::new(file_path);
        let triggers = loader.load().unwrap();

        assert_eq!(triggers.len(), 1);
        let trigger = &triggers[0];
        assert_eq!(trigger.name, "test_telegram");
        if let TriggerTypeConfig::Telegram(config) = &trigger.config {
            assert_eq!(config.token, "123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11");
            assert_eq!(config.chat_id, "-1234567890");
            assert_eq!(config.message.title, "Telegram Title");
        } else {
            panic!("Expected TelegramConfig");
        }
    }

    #[test]
    fn test_load_empty_triggers_file() {
        let yaml_content = "triggers: []";
        let (_temp_dir, file_path) = create_test_dir_with_file("empty_triggers.yaml", yaml_content);
        let loader = TriggerLoader::new(file_path);
        let triggers = loader.load().unwrap();
        assert!(triggers.is_empty());
    }

    #[test]
    fn test_load_non_existent_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("non_existent.yaml");
        let loader = TriggerLoader::new(file_path);
        let err = loader.load().unwrap_err();
        assert!(matches!(err, TriggerLoaderError::IoError(_)));
    }

    #[test]
    fn test_load_unsupported_file_format() {
        let (_temp_dir, file_path) = create_test_dir_with_file("triggers.json", "{}");
        let loader = TriggerLoader::new(file_path);
        let err = loader.load().unwrap_err();
        assert!(matches!(err, TriggerLoaderError::UnsupportedFormat));
    }

    #[test]
    fn test_load_invalid_yaml_syntax() {
        let yaml_content = r###"
triggers:
  - name: invalid
    webhook: { url: "invalid" }
"###;
        let (_temp_dir, file_path) = create_test_dir_with_file("invalid.yaml", yaml_content);
        let loader = TriggerLoader::new(file_path);
        let err = loader.load().unwrap_err();
        assert!(matches!(err, TriggerLoaderError::ParseError(_)));
    }

    #[test]
    fn test_webhook_validation_invalid_url() {
        let webhook_config = WebhookConfig {
            url: "invalid-url".to_string(),
            method: None,
            secret: None,
            headers: None,
            message: NotificationMessage::default(),
            retry_policy: HttpRetryConfig::default(),
        };
        let trigger_type = TriggerTypeConfig::Webhook(webhook_config);
        let err = trigger_type.validate().unwrap_err();
        assert!(matches!(err, NotificationError::ConfigError(_)));
        assert!(err.to_string().contains("Invalid webhook URL"));
    }

    #[test]
    fn test_webhook_validation_empty_title() {
        let webhook_config = WebhookConfig {
            url: "http://example.com".to_string(),
            method: None,
            secret: None,
            headers: None,
            message: NotificationMessage {
                title: "".to_string(),
                ..Default::default()
            },
            retry_policy: HttpRetryConfig::default(),
        };
        let trigger_type = TriggerTypeConfig::Webhook(webhook_config);
        let err = trigger_type.validate().unwrap_err();
        assert!(matches!(err, NotificationError::ConfigError(_)));
        assert!(err.to_string().contains("Webhook title cannot be empty"));
    }

    #[test]
    fn test_slack_validation_invalid_url() {
        let slack_config = SlackConfig {
            slack_url: "invalid-slack-url".to_string(),
            message: NotificationMessage::default(),
            retry_policy: HttpRetryConfig::default(),
        };
        let trigger_type = TriggerTypeConfig::Slack(slack_config);
        let err = trigger_type.validate().unwrap_err();
        assert!(matches!(err, NotificationError::ConfigError(_)));
        assert!(err.to_string().contains("Invalid Slack URL"));
    }

    #[test]
    fn test_slack_validation_incorrect_domain() {
        let slack_config = SlackConfig {
            slack_url: "https://not-slack.com/webhook".to_string(),
            message: NotificationMessage::default(),
            retry_policy: HttpRetryConfig::default(),
        };
        let trigger_type = TriggerTypeConfig::Slack(slack_config);
        let err = trigger_type.validate().unwrap_err();
        assert!(matches!(err, NotificationError::ConfigError(_)));
        assert!(err.to_string().contains("Slack URL appears to be invalid"));
    }

    #[test]
    fn test_discord_validation_invalid_url() {
        let discord_config = DiscordConfig {
            discord_url: "invalid-discord-url".to_string(),
            message: NotificationMessage::default(),
            retry_policy: HttpRetryConfig::default(),
        };
        let trigger_type = TriggerTypeConfig::Discord(discord_config);
        let err = trigger_type.validate().unwrap_err();
        assert!(matches!(err, NotificationError::ConfigError(_)));
        assert!(err.to_string().contains("Invalid Discord URL"));
    }

    #[test]
    fn test_telegram_validation_empty_token() {
        let telegram_config = TelegramConfig {
            token: "".to_string(),
            chat_id: "123".to_string(),
            message: NotificationMessage::default(),
            disable_web_preview: None,
            retry_policy: HttpRetryConfig::default(),
        };
        let trigger_type = TriggerTypeConfig::Telegram(telegram_config);
        let err = trigger_type.validate().unwrap_err();
        assert!(matches!(err, NotificationError::ConfigError(_)));
        assert!(err.to_string().contains("Telegram token cannot be empty"));
    }

    #[test]
    fn test_telegram_validation_empty_chat_id() {
        let telegram_config = TelegramConfig {
            token: "abc".to_string(),
            chat_id: "".to_string(),
            message: NotificationMessage::default(),
            disable_web_preview: None,
            retry_policy: HttpRetryConfig::default(),
        };
        let trigger_type = TriggerTypeConfig::Telegram(telegram_config);
        let err = trigger_type.validate().unwrap_err();
        assert!(matches!(err, NotificationError::ConfigError(_)));
        assert!(err.to_string().contains("Telegram chat_id cannot be empty"));
    }
}
