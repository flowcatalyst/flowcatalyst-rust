//! Message Group Processor
//!
//! Handles FIFO ordering within a message group.
//! OutboxItems with the same message_group are processed sequentially in batches.

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{Mutex, oneshot};
use fc_common::OutboxItem;
use tracing::{debug, error, warn, info};
use async_trait::async_trait;

/// Message dispatch result
#[derive(Debug, Clone)]
pub enum DispatchResult {
    Success,
    Failure { error: String, retryable: bool },
    Blocked { reason: String },
}

/// Result for a single item in a batch
#[derive(Debug, Clone)]
pub struct BatchItemResult {
    pub item_id: String,
    pub result: DispatchResult,
}

/// Batch dispatch result
#[derive(Debug, Clone)]
pub struct BatchDispatchResult {
    pub results: Vec<BatchItemResult>,
}

impl BatchDispatchResult {
    /// Check if all items succeeded
    pub fn all_succeeded(&self) -> bool {
        self.results.iter().all(|r| matches!(r.result, DispatchResult::Success))
    }

    /// Get failed items
    pub fn failed_items(&self) -> Vec<&BatchItemResult> {
        self.results.iter()
            .filter(|r| !matches!(r.result, DispatchResult::Success))
            .collect()
    }
}

/// Single item dispatcher trait
#[async_trait]
pub trait MessageDispatcher: Send + Sync {
    async fn dispatch(&self, item: &OutboxItem) -> DispatchResult;
}

/// Batch dispatcher trait - dispatches multiple outbox items in one API call
#[async_trait]
pub trait BatchMessageDispatcher: Send + Sync {
    async fn dispatch_batch(&self, items: &[OutboxItem]) -> BatchDispatchResult;
}

/// Configuration for message group processor
#[derive(Debug, Clone)]
pub struct MessageGroupProcessorConfig {
    /// Maximum queue depth before blocking
    pub max_queue_depth: usize,
    /// Whether to block on error (stops processing until resolved)
    pub block_on_error: bool,
    /// Maximum retry attempts before giving up
    pub max_retries: u32,
    /// Batch size for API calls (like Java's apiBatchSize)
    pub batch_size: usize,
}

impl Default for MessageGroupProcessorConfig {
    fn default() -> Self {
        Self {
            max_queue_depth: 1000,
            block_on_error: true,
            max_retries: 3,
            batch_size: 100,
        }
    }
}

/// State of the message group processor
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessorState {
    /// Normal processing
    Running,
    /// Blocked due to error (waiting for resolution)
    Blocked { message_id: String, error: String },
    /// Paused by operator
    Paused,
    /// Stopped
    Stopped,
}

/// OutboxItem with tracking info
#[derive(Debug, Clone)]
pub struct TrackedMessage {
    pub item: OutboxItem,
    pub attempt: u32,
    pub last_error: Option<String>,
}

impl TrackedMessage {
    pub fn new(item: OutboxItem) -> Self {
        Self {
            item,
            attempt: 0,
            last_error: None,
        }
    }

    pub fn increment_attempt(&mut self) {
        self.attempt += 1;
    }
}

/// Message group processor - ensures FIFO ordering within a group
pub struct MessageGroupProcessor {
    /// Group identifier
    group_id: String,
    /// Configuration
    config: MessageGroupProcessorConfig,
    /// Item queue
    queue: Arc<Mutex<VecDeque<TrackedMessage>>>,
    /// Current processor state
    state: Arc<Mutex<ProcessorState>>,
    /// Batch dispatcher
    dispatcher: Arc<dyn BatchMessageDispatcher>,
    /// Shutdown signal receiver
    shutdown_rx: Arc<Mutex<Option<oneshot::Receiver<()>>>>,
}

impl MessageGroupProcessor {
    pub fn new(
        group_id: String,
        config: MessageGroupProcessorConfig,
        dispatcher: Arc<dyn BatchMessageDispatcher>,
    ) -> (Self, oneshot::Sender<()>) {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let processor = Self {
            group_id,
            config,
            queue: Arc::new(Mutex::new(VecDeque::new())),
            state: Arc::new(Mutex::new(ProcessorState::Running)),
            dispatcher,
            shutdown_rx: Arc::new(Mutex::new(Some(shutdown_rx))),
        };

        (processor, shutdown_tx)
    }

    /// Get the group ID
    pub fn group_id(&self) -> &str {
        &self.group_id
    }

    /// Enqueue an outbox item for processing
    pub async fn enqueue(&self, item: OutboxItem) -> Result<(), String> {
        let mut queue = self.queue.lock().await;

        if queue.len() >= self.config.max_queue_depth {
            warn!(
                "Queue depth exceeded for group {}, current: {}",
                self.group_id, queue.len()
            );
            return Err("Queue depth exceeded".to_string());
        }

        queue.push_back(TrackedMessage::new(item));
        debug!(
            "Item enqueued for group {}, queue depth: {}",
            self.group_id,
            queue.len()
        );

        Ok(())
    }

    /// Get current queue depth
    pub async fn queue_depth(&self) -> usize {
        let queue = self.queue.lock().await;
        queue.len()
    }

    /// Get current state
    pub async fn state(&self) -> ProcessorState {
        let state = self.state.lock().await;
        state.clone()
    }

    /// Pause processing
    pub async fn pause(&self) {
        let mut state = self.state.lock().await;
        if *state == ProcessorState::Running {
            *state = ProcessorState::Paused;
            info!("Message group processor {} paused", self.group_id);
        }
    }

    /// Resume processing
    pub async fn resume(&self) {
        let mut state = self.state.lock().await;
        if *state == ProcessorState::Paused {
            *state = ProcessorState::Running;
            info!("Message group processor {} resumed", self.group_id);
        }
    }

    /// Unblock the processor (after resolving blocking error)
    pub async fn unblock(&self) {
        let mut state = self.state.lock().await;
        if matches!(*state, ProcessorState::Blocked { .. }) {
            *state = ProcessorState::Running;
            info!("Message group processor {} unblocked", self.group_id);
        }
    }

    /// Skip the blocking item (mark as failed and continue)
    pub async fn skip_blocking_message(&self) -> Option<TrackedMessage> {
        let state_val = self.state().await;
        if !matches!(state_val, ProcessorState::Blocked { .. }) {
            return None;
        }

        let mut queue = self.queue.lock().await;
        let skipped = queue.pop_front();

        let mut state = self.state.lock().await;
        *state = ProcessorState::Running;

        if let Some(ref msg) = skipped {
            info!(
                "Skipped blocking item {} in group {}",
                msg.item.id, self.group_id
            );
        }

        skipped
    }

    /// Process a batch of items from the queue (up to batch_size)
    pub async fn process_batch(&self) -> Option<BatchDispatchResult> {
        // Check state
        let current_state = self.state().await;
        match current_state {
            ProcessorState::Stopped => return None,
            ProcessorState::Paused => return None,
            ProcessorState::Blocked { .. } => return None,
            ProcessorState::Running => {}
        }

        // Drain up to batch_size items
        let mut batch: Vec<TrackedMessage> = {
            let mut queue = self.queue.lock().await;
            let count = queue.len().min(self.config.batch_size);
            if count == 0 {
                return None;
            }
            queue.drain(..count).collect()
        };

        // Increment attempt count for all
        for tracked in &mut batch {
            tracked.increment_attempt();
        }

        debug!(
            "Processing batch of {} items in group {}",
            batch.len(), self.group_id
        );

        // Extract items for dispatch
        let items: Vec<OutboxItem> = batch.iter().map(|t| t.item.clone()).collect();

        // Dispatch batch
        let result = self.dispatcher.dispatch_batch(&items).await;

        // Handle results - process in order to maintain FIFO guarantees
        let mut failed_to_requeue: Vec<TrackedMessage> = Vec::new();
        let mut should_block = false;
        let mut block_item_id = String::new();
        let mut block_error = String::new();

        for (tracked, item_result) in batch.into_iter().zip(result.results.iter()) {
            match &item_result.result {
                DispatchResult::Success => {
                    debug!(
                        "Item {} dispatched successfully in group {}",
                        tracked.item.id, self.group_id
                    );
                }
                DispatchResult::Failure { error, retryable } => {
                    let mut tracked = tracked;
                    tracked.last_error = Some(error.clone());

                    if *retryable && tracked.attempt < self.config.max_retries {
                        failed_to_requeue.push(tracked);
                        debug!(
                            "Item {} will be re-queued for retry in group {}",
                            item_result.item_id, self.group_id
                        );
                    } else if self.config.block_on_error {
                        if !should_block {
                            should_block = true;
                            block_item_id = item_result.item_id.clone();
                            block_error = error.clone();
                        }
                        failed_to_requeue.push(tracked);
                    } else {
                        error!(
                            "Item {} failed permanently in group {}: {}",
                            item_result.item_id, self.group_id, error
                        );
                    }
                }
                DispatchResult::Blocked { reason } => {
                    if !should_block {
                        should_block = true;
                        block_item_id = item_result.item_id.clone();
                        block_error = reason.clone();
                    }
                    failed_to_requeue.push(tracked);
                }
            }
        }

        // Re-queue failed items at the front (in reverse order to maintain FIFO)
        if !failed_to_requeue.is_empty() {
            let mut queue = self.queue.lock().await;
            for tracked in failed_to_requeue.into_iter().rev() {
                queue.push_front(tracked);
            }
        }

        // Set blocked state if needed
        if should_block {
            let mut state = self.state.lock().await;
            *state = ProcessorState::Blocked {
                message_id: block_item_id.clone(),
                error: block_error.clone(),
            };
            error!(
                "Message group processor {} blocked on item {}",
                self.group_id, block_item_id
            );
        }

        Some(result)
    }

    /// Process one item from the queue (for backwards compatibility)
    pub async fn process_one(&self) -> Option<DispatchResult> {
        let batch_result = self.process_batch().await?;
        batch_result.results.into_iter().next().map(|r| r.result)
    }

    /// Run the processor loop
    pub async fn run(&self) {
        info!("Starting message group processor for {}", self.group_id);

        let mut shutdown_rx = {
            let mut rx = self.shutdown_rx.lock().await;
            rx.take()
        };

        loop {
            // Check for shutdown
            if let Some(ref mut rx) = shutdown_rx {
                if rx.try_recv().is_ok() {
                    let mut state = self.state.lock().await;
                    *state = ProcessorState::Stopped;
                    break;
                }
            }

            // Process batch
            match self.process_batch().await {
                Some(_) => {
                    // Continue immediately if we processed something
                }
                None => {
                    // Nothing to process, wait a bit
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                }
            }
        }

        info!("Message group processor {} stopped", self.group_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_common::OutboxStatus;
    use chrono::Utc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockBatchDispatcher {
        success_count: AtomicUsize,
        fail_until: AtomicUsize,
    }

    impl MockBatchDispatcher {
        fn new(fail_until: usize) -> Self {
            Self {
                success_count: AtomicUsize::new(0),
                fail_until: AtomicUsize::new(fail_until),
            }
        }
    }

    #[async_trait]
    impl BatchMessageDispatcher for MockBatchDispatcher {
        async fn dispatch_batch(&self, items: &[OutboxItem]) -> BatchDispatchResult {
            let results = items.iter().map(|item| {
                let current = self.success_count.fetch_add(1, Ordering::SeqCst);
                let result = if current < self.fail_until.load(Ordering::SeqCst) {
                    DispatchResult::Failure {
                        error: "Mock failure".to_string(),
                        retryable: true,
                    }
                } else {
                    DispatchResult::Success
                };
                BatchItemResult {
                    item_id: item.id.clone(),
                    result,
                }
            }).collect();
            BatchDispatchResult { results }
        }
    }

    fn create_test_item(id: &str) -> OutboxItem {
        OutboxItem {
            id: id.to_string(),
            item_type: fc_common::OutboxItemType::EVENT,
            message_group: Some("group-1".to_string()),
            payload: serde_json::json!({"test": true}),
            status: OutboxStatus::IN_PROGRESS,
            retry_count: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            error_message: None,
            client_id: None,
            payload_size: None,
            headers: None,
        }
    }

    #[tokio::test]
    async fn test_enqueue_and_process() {
        let dispatcher = Arc::new(MockBatchDispatcher::new(0));
        let config = MessageGroupProcessorConfig {
            batch_size: 1,
            ..Default::default()
        };
        let (processor, _shutdown) = MessageGroupProcessor::new(
            "test-group".to_string(),
            config,
            dispatcher,
        );

        processor.enqueue(create_test_item("msg-1")).await.unwrap();
        processor.enqueue(create_test_item("msg-2")).await.unwrap();

        assert_eq!(processor.queue_depth().await, 2);

        let result1 = processor.process_one().await;
        assert!(matches!(result1, Some(DispatchResult::Success)));
        assert_eq!(processor.queue_depth().await, 1);

        let result2 = processor.process_one().await;
        assert!(matches!(result2, Some(DispatchResult::Success)));
        assert_eq!(processor.queue_depth().await, 0);
    }

    #[tokio::test]
    async fn test_batch_processing() {
        let dispatcher = Arc::new(MockBatchDispatcher::new(0));
        let config = MessageGroupProcessorConfig {
            batch_size: 10,
            ..Default::default()
        };
        let (processor, _shutdown) = MessageGroupProcessor::new(
            "test-group".to_string(),
            config,
            dispatcher,
        );

        for i in 0..5 {
            processor.enqueue(create_test_item(&format!("msg-{}", i))).await.unwrap();
        }
        assert_eq!(processor.queue_depth().await, 5);

        let result = processor.process_batch().await;
        assert!(result.is_some());
        let batch_result = result.unwrap();
        assert_eq!(batch_result.results.len(), 5);
        assert!(batch_result.all_succeeded());
        assert_eq!(processor.queue_depth().await, 0);
    }

    #[tokio::test]
    async fn test_block_on_error() {
        let dispatcher = Arc::new(MockBatchDispatcher::new(10));
        let config = MessageGroupProcessorConfig {
            max_retries: 2,
            block_on_error: true,
            batch_size: 1,
            ..Default::default()
        };
        let (processor, _shutdown) = MessageGroupProcessor::new(
            "test-group".to_string(),
            config,
            dispatcher,
        );

        processor.enqueue(create_test_item("msg-1")).await.unwrap();

        for _ in 0..2 {
            let _ = processor.process_one().await;
        }

        let state = processor.state().await;
        assert!(matches!(state, ProcessorState::Blocked { .. }));

        processor.unblock().await;
        assert_eq!(processor.state().await, ProcessorState::Running);
    }

    #[tokio::test]
    async fn test_pause_resume() {
        let dispatcher = Arc::new(MockBatchDispatcher::new(0));
        let config = MessageGroupProcessorConfig {
            batch_size: 1,
            ..Default::default()
        };
        let (processor, _shutdown) = MessageGroupProcessor::new(
            "test-group".to_string(),
            config,
            dispatcher,
        );

        processor.enqueue(create_test_item("msg-1")).await.unwrap();

        processor.pause().await;
        assert_eq!(processor.state().await, ProcessorState::Paused);

        let result = processor.process_one().await;
        assert!(result.is_none());
        assert_eq!(processor.queue_depth().await, 1);

        processor.resume().await;
        assert_eq!(processor.state().await, ProcessorState::Running);

        let result = processor.process_one().await;
        assert!(matches!(result, Some(DispatchResult::Success)));
    }
}
