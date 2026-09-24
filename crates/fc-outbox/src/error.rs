//! Errors raised while routing outbox items through the group processors.

/// Failure to hand an outbox item to its message-group processor, or to act
/// on a group by id.
///
/// The `Display` text of the dispatch variants is the upstream error text
/// unchanged, because the processor stores it as the row's `error_message`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OutboxError {
    /// The group's in-memory queue is at `max_queue_depth`.
    #[error("Queue depth exceeded")]
    QueueFull,
    /// No idle group could be evicted to make room for a new one.
    #[error("Maximum group count reached")]
    MaxGroupsReached,
    /// No processor is running for this message group.
    #[error("Group {0} not found")]
    GroupNotFound(String),
    /// A group-less item was dispatched and the platform rejected it.
    #[error("{error}")]
    DispatchFailed { error: String, retryable: bool },
    /// A group-less item was dispatched and reported as blocked.
    #[error("{reason}")]
    Blocked { reason: String },
    /// The dispatcher returned no result for the item it was given.
    #[error("No result from dispatch")]
    NoDispatchResult,
}
