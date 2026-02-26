use thiserror::Error;

#[derive(Error, Debug)]
pub enum QueueError {
    #[error("Database error: {0}")]
    Database(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Message not found: {0}")]
    NotFound(String),

    #[error("Visibility timeout exceeded")]
    VisibilityTimeout,

    #[error("Queue is stopped")]
    Stopped,

    #[error("AWS SQS error: {0}")]
    Sqs(String),

    #[error("NATS error: {0}")]
    Nats(String),

    #[error("Configuration error: {0}")]
    Config(String),
}

#[cfg(feature = "sqlite")]
impl From<sqlx::Error> for QueueError {
    fn from(e: sqlx::Error) -> Self {
        QueueError::Database(e.to_string())
    }
}
