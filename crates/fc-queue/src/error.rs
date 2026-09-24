use thiserror::Error;

/// A type-erased, thread-safe error. Used for backends whose client
/// libraries return a different generic error type per operation (the AWS
/// SDK's `SdkError<OpError, _>`, async-nats' `Error<Kind>`), so a single
/// variant can still carry the original error as its `source()`.
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Error, Debug)]
pub enum QueueError {
    #[cfg(any(feature = "sqlite", feature = "postgres"))]
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Message not found: {0}")]
    NotFound(String),

    #[error("Visibility timeout exceeded")]
    VisibilityTimeout,

    #[error("Queue is stopped")]
    Stopped,

    /// The backend connection has not been established (or was torn down).
    #[error("Not connected")]
    NotConnected,

    /// A message received from the broker is missing a required part.
    #[error("Invalid message: {0}")]
    InvalidMessage(&'static str),

    #[cfg(feature = "activemq")]
    #[error("AMQP error: {context}: {source}")]
    Amqp {
        context: &'static str,
        #[source]
        source: lapin::Error,
    },

    #[cfg(feature = "sqs")]
    #[error("AWS SQS error: {0}")]
    Sqs(#[source] BoxError),

    #[cfg(feature = "nats")]
    #[error("NATS error: {context}: {source}")]
    Nats {
        context: String,
        #[source]
        source: BoxError,
    },

    #[error("Configuration error: {0}")]
    Config(String),
}

impl QueueError {
    #[cfg(feature = "activemq")]
    pub(crate) fn amqp(context: &'static str, source: lapin::Error) -> Self {
        QueueError::Amqp { context, source }
    }

    #[cfg(feature = "sqs")]
    pub fn sqs(source: impl Into<BoxError>) -> Self {
        QueueError::Sqs(source.into())
    }

    #[cfg(feature = "nats")]
    pub(crate) fn nats(context: impl Into<String>, source: impl Into<BoxError>) -> Self {
        QueueError::Nats {
            context: context.into(),
            source: source.into(),
        }
    }
}
