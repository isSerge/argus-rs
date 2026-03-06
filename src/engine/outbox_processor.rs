//! Service responsible for processing the persistent action queue (outbox
//! pattern).

use std::{sync::Arc, time::Duration};

use futures::{StreamExt, stream};
use tokio_util::sync::CancellationToken;

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
    pub async fn run(&self, cancellation_token: CancellationToken) {
        tracing::info!("OutboxProcessor started.");

        loop {
            // Calculate delay for next poll
            let delay = tokio::time::sleep(Duration::from_millis(self.config.poll_interval_ms));

            tokio::select! {
                // 1. Check for shutdown signal
                _ = cancellation_token.cancelled() => {
                    tracing::info!("OutboxProcessor received shutdown signal. Stopping loop.");
                    break;
                }

                // 2. Wait for poll interval
                _ = delay => {
                    if let Err(e) = self.process_batch().await {
                        tracing::error!("Error in outbox processing loop: {}", e);
                    }
                }
            }
        }

        tracing::info!("OutboxProcessor has shut down.");
    }

    /// Drains the outbox queue by processing all pending items until the queue
    /// is empty. Used for shutdown and dry-run modes to ensure all pending
    /// actions are attempted before exiting. Returns the count of items
    /// successfully delivered (removed from outbox).
    pub async fn drain_queue(&self) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let mut total_successful = 0;

        loop {
            let items = self.state.get_pending_outbox(self.config.batch_size).await?;

            if items.is_empty() {
                break;
            }

            let successful_count = self.process_batch_items(items).await?;
            total_successful += successful_count;
        }

        tracing::info!("Outbox drained. {} items successfully delivered.", total_successful);

        Ok(total_successful)
    }

    /// Processes a batch of outbox items concurrently, handling success and
    /// failure cases. On success, the item is deleted from the outbox. On
    /// failure, the retry count is incremented. Returns the count of items
    /// successfully deleted. This method is used by both the regular processing
    /// loop and the drain_queue method to ensure consistent handling of outbox
    /// items.
    async fn process_batch_items(
        &self,
        items: Vec<OutboxItem>,
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
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
        let concurrency = self.config.concurrency.max(1);
        let mut results = tasks.buffer_unordered(concurrency);

        let mut successful_count = 0;

        // Handle results and update state accordingly
        while let Some((id, result)) = results.next().await {
            match result {
                Ok(_) => {
                    // Success: Delete the item
                    if let Err(e) = self.state.delete_outbox_item(id).await {
                        tracing::error!("Failed to delete outbox item {} after success: {}", id, e);
                    } else {
                        successful_count += 1;
                    }
                }
                Err(e) => {
                    // Failure: Increment retry count
                    tracing::warn!("Failed to deliver outbox item {}: {}. Retrying later.", id, e);
                    let _ = self.state.increment_outbox_retries(id).await;
                }
            }
        }

        Ok(successful_count)
    }

    /// Processes a batch of pending outbox items.
    pub async fn process_batch(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Fetch a batch of pending items (e.g., 50 at a time)
        let items = self.state.get_pending_outbox(self.config.batch_size).await?;

        if items.is_empty() {
            return Ok(()); // No items to process
        }

        let _ = self.process_batch_items(items).await?; // Discard count, not needed in the loop
        Ok(())
    }
}
