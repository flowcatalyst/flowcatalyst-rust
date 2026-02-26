//! Error types for the standby module

use thiserror::Error;

#[derive(Error, Debug)]
pub enum StandbyError {
    #[error("Redis connection error: {0}")]
    Connection(String),

    #[error("Redis operation error: {0}")]
    Redis(#[from] redis::RedisError),

    #[error("Lock acquisition failed: {0}")]
    LockFailed(String),

    #[error("Leader election not started")]
    NotStarted,

    #[error("Already running")]
    AlreadyRunning,

    #[error("Configuration error: {0}")]
    Config(String),
}

pub type Result<T> = std::result::Result<T, StandbyError>;
