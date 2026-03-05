//! Service responsible for processing the persistent action queue (outbox
//! pattern).

use std::{sync::Arc, time::Duration};

use futures::{StreamExt, stream};
use tokio::time::sleep;

use crate::{
    action_dispatcher::ActionDispatcher,
    config::OutboxConfig,
    persistence::traits::{AppRepository, OutboxItem},
};

/// Service responsible for processing the persistent action queue.
pub struct OutboxProcessor<S: AppRepository + ?Sized> {
    state: Arc<S>,
    dispatcher: Arc<ActionDispatcher>,
    config: OutboxConfig,
}

impl<S: AppRepository + ?Sized + Send + Sync + 'static> OutboxProcessor<S> {
    /// Creates a new OutboxProcessor instance.
    pub fn new(state: Arc<S>, dispatcher: Arc<ActionDispatcher>, config: OutboxConfig) -> Self {
        Self { state, dispatcher, config }
    }

    /// Starts the outbox processing loop.
    pub async fn run(self) {
        tracing::info!("OutboxProcessor started.");
        loop {
            if let Err(e) = self.process_batch().await {
                tracing::error!("Error in outbox processing loop: {}", e);
            }
            sleep(Duration::from_millis(self.config.poll_interval_ms)).await;
        }
    }

    /// Drains the outbox queue by processing all pending items until the queue
    /// is empty. Used for shutdown and dry-run modes to ensure all pending
    /// actions are attempted before exiting.
    pub async fn drain_queue(&self) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let mut total_processed = 0;

        loop {
            let items = self.state.get_pending_outbox(self.config.batch_size).await?;

            if items.is_empty() {
                break;
            }

            let count = items.len();

            self.process_batch_items(items).await?;
            total_processed += count;
        }

        tracing::info!("Outbox drained. Total items processed: {}", total_processed);

        Ok(total_processed)
    }

    /// Processes a batch of outbox items concurrently, handling success and
    /// failure cases. On success, the item is deleted from the outbox. On
    /// failure, the retry count is incremented. This method is used by both
    /// the regular processing loop and the drain_queue method to ensure
    /// consistent handling of outbox items.
    async fn process_batch_items(
        &self,
        items: Vec<OutboxItem>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Create a stream of async tasks for processing each item concurrently
        let tasks = stream::iter(items).map(|item| {
            let dispatcher = self.dispatcher.clone();
            async move {
                let id = item.id;
                let result = dispatcher.execute(item.payload).await;
                // Return a tuple: (ID, Result)
                (id, result)
            }
        });

        // Execute concurrently with a limit (e.g., 10 parallel requests)
        let mut results = tasks.buffer_unordered(self.config.concurrency);

        // Handle results and update state accordingly
        while let Some((id, result)) = results.next().await {
            match result {
                Ok(_) => {
                    // Success: Delete the item
                    if let Err(e) = self.state.delete_outbox_item(id).await {
                        tracing::error!("Failed to delete outbox item {} after success: {}", id, e);
                    }
                }
                Err(e) => {
                    // Failure: Increment retry count
                    tracing::warn!("Failed to deliver outbox item {}: {}. Retrying later.", id, e);
                    let _ = self.state.increment_outbox_retries(id).await;
                }
            }
        }

        Ok(())
    }

    /// Processes a batch of pending outbox items.
    pub async fn process_batch(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Fetch a batch of pending items (e.g., 50 at a time)
        let items = self.state.get_pending_outbox(self.config.batch_size).await?;

        if items.is_empty() {
            return Ok(()); // No items to process
        }

        self.process_batch_items(items).await
    }
}
