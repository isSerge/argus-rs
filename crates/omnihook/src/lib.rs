//! `omnihook` — generic webhook client with HMAC signing and platform-specific
//! payload builders for Slack, Discord, Telegram, and generic endpoints.

pub mod client;
pub mod error;
pub mod payload_builder;

pub use client::{WebhookClient, WebhookConfig};
pub use error::OmnihookError;
pub use payload_builder::{
    DiscordPayloadBuilder, GenericWebhookPayloadBuilder, SlackPayloadBuilder,
    TelegramPayloadBuilder, WebhookPayloadBuilder,
};
