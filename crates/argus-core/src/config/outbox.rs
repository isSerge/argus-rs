use serde::Deserialize;

/// Configuration for the outbox processor service.
#[derive(Debug, Deserialize, Clone)]
pub struct OutboxConfig {
    /// How many pending outbox items to fetch from the DB at once.
    #[serde(default = "default_outbox_batch_size")]
    pub batch_size: i64,

    /// How many outbox items to send concurrently.
    #[serde(default = "default_outbox_concurrency")]
    pub concurrency: usize,

    /// How often to check the DB for new outbox items (in ms).
    #[serde(default = "default_outbox_poll_interval")]
    pub poll_interval_ms: u64,
}

fn default_outbox_batch_size() -> i64 {
    100
}
fn default_outbox_concurrency() -> usize {
    10
}
fn default_outbox_poll_interval() -> u64 {
    500
}

impl Default for OutboxConfig {
    fn default() -> Self {
        Self {
            batch_size: default_outbox_batch_size(),
            concurrency: default_outbox_concurrency(),
            poll_interval_ms: default_outbox_poll_interval(),
        }
    }
}
