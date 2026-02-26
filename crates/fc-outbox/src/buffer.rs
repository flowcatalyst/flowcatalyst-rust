//! Global Buffer
//!
//! A thread-safe buffer that collects outbox items from multiple sources before distribution.
//!
//! When the buffer is full, items are rejected (not dropped). The item remains
//! in its IN_PROGRESS state in the database and will be recovered by the crash recovery
//! task after the configured timeout.

use std::collections::VecDeque;
use std::sync::Arc;
use std::fmt;
use tokio::sync::{Mutex, mpsc};
use fc_common::OutboxItem;
use tracing::{debug, warn};

/// Error returned when the buffer is full.
///
/// This is NOT a data loss scenario - the item remains in IN_PROGRESS state
/// in the database and will be recovered by the crash recovery task.
#[derive(Debug, Clone)]
pub struct BufferFullError {
    pub item_id: String,
}

impl fmt::Display for BufferFullError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Buffer full, item {} rejected (will be recovered from IN_PROGRESS state)",
            self.item_id
        )
    }
}

impl std::error::Error for BufferFullError {}

/// Global buffer configuration
#[derive(Debug, Clone)]
pub struct GlobalBufferConfig {
    /// Maximum buffer size. When exceeded, new items are rejected (not dropped).
    /// Rejected items remain in IN_PROGRESS state and are recovered by crash recovery.
    pub max_size: usize,
    /// Batch size for draining
    pub batch_size: usize,
}

impl Default for GlobalBufferConfig {
    fn default() -> Self {
        Self {
            max_size: 10000,
            batch_size: 100,
        }
    }
}

/// Global buffer for collecting outbox items before distribution
pub struct GlobalBuffer {
    config: GlobalBufferConfig,
    buffer: Arc<Mutex<VecDeque<OutboxItem>>>,
    sender: mpsc::Sender<OutboxItem>,
    receiver: Arc<Mutex<mpsc::Receiver<OutboxItem>>>,
}

impl GlobalBuffer {
    pub fn new(config: GlobalBufferConfig) -> Self {
        let (sender, receiver) = mpsc::channel(config.max_size);
        Self {
            config,
            buffer: Arc::new(Mutex::new(VecDeque::new())),
            sender,
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }

    /// Get a sender handle for pushing items
    pub fn sender(&self) -> mpsc::Sender<OutboxItem> {
        self.sender.clone()
    }

    /// Push an item to the buffer.
    ///
    /// Returns `BufferFullError` if the buffer is at capacity. This is NOT a data loss
    /// scenario - the item remains in IN_PROGRESS state in the database and will be
    /// recovered by the crash recovery task after the configured timeout.
    pub async fn push(&self, item: OutboxItem) -> Result<(), BufferFullError> {
        let mut buffer = self.buffer.lock().await;
        if buffer.len() >= self.config.max_size {
            warn!(
                "Global buffer full (capacity: {}), item {} rejected - will be recovered from IN_PROGRESS state",
                self.config.max_size,
                item.id
            );
            return Err(BufferFullError {
                item_id: item.id,
            });
        }
        buffer.push_back(item);
        debug!("Item added to global buffer, size: {}", buffer.len());
        Ok(())
    }

    /// Drain up to batch_size items from the buffer
    pub async fn drain_batch(&self) -> Vec<OutboxItem> {
        let mut buffer = self.buffer.lock().await;
        let count = buffer.len().min(self.config.batch_size);
        let mut batch = Vec::with_capacity(count);
        for _ in 0..count {
            if let Some(item) = buffer.pop_front() {
                batch.push(item);
            }
        }
        debug!("Drained {} items from global buffer", batch.len());
        batch
    }

    /// Get current buffer size
    pub async fn len(&self) -> usize {
        let buffer = self.buffer.lock().await;
        buffer.len()
    }

    /// Check if buffer is empty
    pub async fn is_empty(&self) -> bool {
        let buffer = self.buffer.lock().await;
        buffer.is_empty()
    }

    /// Process incoming items from the channel
    pub async fn process_incoming(&self) {
        let mut receiver = self.receiver.lock().await;
        while let Some(item) = receiver.recv().await {
            let _ = self.push(item).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_common::OutboxStatus;
    use chrono::Utc;

    fn create_test_item(id: &str) -> OutboxItem {
        OutboxItem {
            id: id.to_string(),
            item_type: fc_common::OutboxItemType::EVENT,
            message_group: None,
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
    async fn test_push_and_drain() {
        let config = GlobalBufferConfig {
            max_size: 100,
            batch_size: 10,
        };
        let buffer = GlobalBuffer::new(config);

        // Push items
        for i in 0..25 {
            buffer.push(create_test_item(&format!("msg-{}", i))).await.unwrap();
        }

        assert_eq!(buffer.len().await, 25);

        // Drain first batch
        let batch1 = buffer.drain_batch().await;
        assert_eq!(batch1.len(), 10);
        assert_eq!(buffer.len().await, 15);

        // Drain second batch
        let batch2 = buffer.drain_batch().await;
        assert_eq!(batch2.len(), 10);
        assert_eq!(buffer.len().await, 5);

        // Drain remaining
        let batch3 = buffer.drain_batch().await;
        assert_eq!(batch3.len(), 5);
        assert!(buffer.is_empty().await);
    }

    #[tokio::test]
    async fn test_buffer_overflow() {
        let config = GlobalBufferConfig {
            max_size: 5,
            batch_size: 2,
        };
        let buffer = GlobalBuffer::new(config);

        // Fill buffer
        for i in 0..5 {
            buffer.push(create_test_item(&format!("msg-{}", i))).await.unwrap();
        }

        // Should fail on overflow
        let result = buffer.push(create_test_item("overflow")).await;
        assert!(result.is_err());
    }
}
