//! # Webhook Payload Builder
//!
//! Traits and implementations for constructing channel-specific JSON payloads
//! for Slack, Discord, Telegram, and generic webhooks.

use regex::Regex;
use serde_json::json;

/// A trait for building channel-specific webhook payloads.
pub trait WebhookPayloadBuilder: Send + Sync {
    /// Builds a webhook payload from a title and rendered body string.
    fn build_payload(&self, title: &str, body: &str) -> serde_json::Value;
}

/// A payload builder for Slack notifications.
///
/// Creates a `blocks`-based section with mrkdwn-formatted text.
pub struct SlackPayloadBuilder;

impl WebhookPayloadBuilder for SlackPayloadBuilder {
    fn build_payload(&self, title: &str, body: &str) -> serde_json::Value {
        let full_message = format!("*{title}*\n\n{body}");
        json!({
            "blocks": [
                {
                    "type": "section",
                    "text": {
                        "type": "mrkdwn",
                        "text": full_message
                    }
                }
            ]
        })
    }
}

/// A payload builder for Discord notifications.
///
/// Creates a simple `content` field with markdown-formatted text.
pub struct DiscordPayloadBuilder;

impl WebhookPayloadBuilder for DiscordPayloadBuilder {
    fn build_payload(&self, title: &str, body: &str) -> serde_json::Value {
        let full_message = format!("*{title}*\n\n{body}");
        json!({
            "content": full_message
        })
    }
}

/// A payload builder for Telegram notifications using MarkdownV2.
pub struct TelegramPayloadBuilder {
    /// The chat ID to send the message to.
    pub chat_id: String,
    /// Whether to disable web page previews in the message.
    pub disable_web_preview: bool,
}

impl TelegramPayloadBuilder {
    /// Escapes a string for Telegram's MarkdownV2 format.
    ///
    /// Preserves existing markdown entities (bold, italic, links, code blocks)
    /// while escaping special characters outside of them.
    fn escape_markdown_v2(text: &str) -> String {
        const SPECIAL: &[char] = &[
            '_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.',
            '!', '\\',
        ];

        let re =
            Regex::new(r"(?s)```.*?```|`[^`]*`|\*[^*]*\*|_[^_]*_|~[^~]*~|\[([^\]]+)\]\(([^)]+)\)")
                .unwrap();

        let mut out = String::with_capacity(text.len());
        let mut last = 0;

        for caps in re.captures_iter(text) {
            let mat = caps.get(0).unwrap();

            for c in text[last..mat.start()].chars() {
                if SPECIAL.contains(&c) {
                    out.push('\\');
                }
                out.push(c);
            }

            if let (Some(lbl), Some(url)) = (caps.get(1), caps.get(2)) {
                let mut esc_label = String::with_capacity(lbl.as_str().len() * 2);
                for c in lbl.as_str().chars() {
                    if SPECIAL.contains(&c) {
                        esc_label.push('\\');
                    }
                    esc_label.push(c);
                }
                let mut esc_url = String::with_capacity(url.as_str().len() * 2);
                for c in url.as_str().chars() {
                    if SPECIAL.contains(&c) {
                        esc_url.push('\\');
                    }
                    esc_url.push(c);
                }
                out.push('[');
                out.push_str(&esc_label);
                out.push(']');
                out.push('(');
                out.push_str(&esc_url);
                out.push(')');
            } else {
                out.push_str(mat.as_str());
            }

            last = mat.end();
        }

        for c in text[last..].chars() {
            if SPECIAL.contains(&c) {
                out.push('\\');
            }
            out.push(c);
        }

        out
    }
}

impl WebhookPayloadBuilder for TelegramPayloadBuilder {
    fn build_payload(&self, title: &str, body: &str) -> serde_json::Value {
        let escaped_title = Self::escape_markdown_v2(title);
        let escaped_message = Self::escape_markdown_v2(body);

        let full_message = format!("*{escaped_title}* \n\n{escaped_message}");
        json!({
            "chat_id": self.chat_id,
            "text": full_message,
            "parse_mode": "MarkdownV2",
            "disable_web_page_preview": self.disable_web_preview
        })
    }
}

/// A payload builder for generic webhooks. Produces a simple `{title, body}`
/// JSON object.
pub struct GenericWebhookPayloadBuilder;

impl WebhookPayloadBuilder for GenericWebhookPayloadBuilder {
    fn build_payload(&self, title: &str, body: &str) -> serde_json::Value {
        json!({
            "title": title,
            "body": body
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_slack_payload_builder() {
        let payload = SlackPayloadBuilder.build_payload("Test Title", "Test Message");
        assert_eq!(
            payload,
            json!({
                "blocks": [
                    {
                        "type": "section",
                        "text": {
                            "type": "mrkdwn",
                            "text": "*Test Title*\n\nTest Message"
                        }
                    }
                ]
            })
        );
    }

    #[test]
    fn test_discord_payload_builder() {
        let payload = DiscordPayloadBuilder.build_payload("Test Title", "Test Message");
        assert_eq!(payload, json!({ "content": "*Test Title*\n\nTest Message" }));
    }

    #[test]
    fn test_telegram_payload_builder() {
        let builder =
            TelegramPayloadBuilder { chat_id: "12345".to_string(), disable_web_preview: true };
        let payload = builder.build_payload("Test Title", "Test Message");
        assert_eq!(
            payload,
            json!({
                "chat_id": "12345",
                "text": "*Test Title* \n\nTest Message",
                "parse_mode": "MarkdownV2",
                "disable_web_page_preview": true
            })
        );
    }

    #[test]
    fn test_generic_webhook_payload_builder() {
        let payload = GenericWebhookPayloadBuilder.build_payload("Test Title", "Test Message");
        assert_eq!(payload, json!({ "title": "Test Title", "body": "Test Message" }));
    }

    #[test]
    fn test_escape_markdown_v2() {
        assert_eq!(
            TelegramPayloadBuilder::escape_markdown_v2(
                "*Transaction Alert*\n*Network:* Base Sepolia\n*From:* 0x00001\n*To:* 0x00002\n*Transaction:* [View on Blockscout](https://base-sepolia.blockscout.com/tx/0x00003)"
            ),
            "*Transaction Alert*\n*Network:* Base Sepolia\n*From:* 0x00001\n*To:* 0x00002\n*Transaction:* [View on Blockscout](https://base\\-sepolia\\.blockscout\\.com/tx/0x00003)"
        );
        assert_eq!(
            TelegramPayloadBuilder::escape_markdown_v2("Hello *world*!"),
            "Hello *world*\\!"
        );
        assert_eq!(
            TelegramPayloadBuilder::escape_markdown_v2("(test) [test] {test} <test>"),
            "\\(test\\) \\[test\\] \\{test\\} <test\\>"
        );
        assert_eq!(
            TelegramPayloadBuilder::escape_markdown_v2("```code block```"),
            "```code block```"
        );
        assert_eq!(TelegramPayloadBuilder::escape_markdown_v2("`inline code`"), "`inline code`");
        assert_eq!(TelegramPayloadBuilder::escape_markdown_v2("*bold text*"), "*bold text*");
        assert_eq!(TelegramPayloadBuilder::escape_markdown_v2("_italic text_"), "_italic text_");
        assert_eq!(
            TelegramPayloadBuilder::escape_markdown_v2("~strikethrough~"),
            "~strikethrough~"
        );
        assert_eq!(
            TelegramPayloadBuilder::escape_markdown_v2("[link](https://example.com/test.html)"),
            "[link](https://example\\.com/test\\.html)"
        );
        assert_eq!(
            TelegramPayloadBuilder::escape_markdown_v2(
                "[test!*_]{link}](https://test.com/path[1])"
            ),
            "\\[test\\!\\*\\_\\]\\{link\\}\\]\\(https://test\\.com/path\\[1\\]\\)"
        );
        assert_eq!(
            TelegramPayloadBuilder::escape_markdown_v2(
                "Hello *bold* and [link](http://test.com) and `code`"
            ),
            "Hello *bold* and [link](http://test\\.com) and `code`"
        );
        assert_eq!(TelegramPayloadBuilder::escape_markdown_v2("test\\test"), "test\\\\test");
        assert_eq!(
            TelegramPayloadBuilder::escape_markdown_v2("_*[]()~`>#+-=|{}.!\\"),
            "\\_\\*\\[\\]\\(\\)\\~\\`\\>\\#\\+\\-\\=\\|\\{\\}\\.\\!\\\\",
        );
        assert_eq!(
            TelegramPayloadBuilder::escape_markdown_v2("*bold with [link](http://test.com)*"),
            "*bold with [link](http://test.com)*"
        );
        assert_eq!(TelegramPayloadBuilder::escape_markdown_v2(""), "");
        assert_eq!(TelegramPayloadBuilder::escape_markdown_v2("***"), "**\\*");
    }
}
