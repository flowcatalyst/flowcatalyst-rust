//! Errors returned by the simple outbox API ([`OutboxManager`](super::OutboxManager),
//! [`OutboxDriver`](super::OutboxDriver), the payload writers and schema setup).

/// Failure to write outbox rows.
#[derive(Debug, thiserror::Error)]
pub enum OutboxError {
    /// The [`OutboxManager`](super::OutboxManager) was built with an empty client id.
    #[error(
        "OutboxManager: client_id is required. Provide a valid client ID when constructing the OutboxManager."
    )]
    MissingClientId,

    /// A payload could not be serialized to JSON.
    #[error("outbox payload serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),

    /// The database rejected a statement (built-in sqlx driver and writers).
    #[error("outbox database error: {0}")]
    Database(#[from] sqlx::Error),

    /// A custom [`OutboxDriver`](super::OutboxDriver) failed; box the native error here.
    #[error("outbox driver error: {0}")]
    Driver(#[source] Box<dyn std::error::Error + Send + Sync>),
}
