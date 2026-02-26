use thiserror::Error;

#[derive(Error, Debug)]
pub enum RouterError {
    #[error("Pool error: {0}")]
    Pool(String),

    #[error("Queue error: {0}")]
    Queue(String),

    #[error("Mediation error: {0}")]
    Mediation(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Pool not found: {0}")]
    PoolNotFound(String),

    #[error("Pool at capacity: {0}")]
    PoolAtCapacity(String),

    #[error("Rate limited: {0}")]
    RateLimited(String),

    #[error("Shutdown in progress")]
    ShutdownInProgress,

    #[error("Duplicate message: {0}")]
    DuplicateMessage(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
