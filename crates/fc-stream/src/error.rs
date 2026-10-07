//! What the stream processors report when a cycle fails.
//!
//! Each variant's `Display` is the text of the `anyhow` error it replaces;
//! the processors log it (`error!(error = %e, ..)`) and count it in
//! [`crate::health::StreamHealth`].

use std::result;

/// A stream-processor cycle failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A query or transaction against the platform database failed.
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    /// The dispatch-job projection's upsert failed.
    #[error("dispatch job projection query failed: {0}")]
    DispatchJobProjection(#[source] sqlx::Error),
    /// The event projection's upsert failed.
    #[error("event projection query failed: {0}")]
    EventProjection(#[source] sqlx::Error),
}

pub type Result<T, E = Error> = result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::Error;
    use std::error::Error as _;

    // The expected strings are what `anyhow::anyhow!("... {}", e)` produced.
    #[test]
    fn the_projection_messages_are_unchanged() {
        let e = Error::DispatchJobProjection(sqlx::Error::PoolTimedOut);
        assert_eq!(
            e.to_string(),
            "dispatch job projection query failed: pool timed out while waiting for an open connection"
        );
        assert!(e.source().is_some());
        let e = Error::EventProjection(sqlx::Error::PoolClosed);
        assert_eq!(
            e.to_string(),
            "event projection query failed: attempted to acquire a connection on a closed pool"
        );
        assert!(e.source().is_some());
    }

    #[test]
    fn a_database_error_renders_as_the_driver_does() {
        let e = Error::from(sqlx::Error::PoolClosed);
        assert_eq!(e.to_string(), sqlx::Error::PoolClosed.to_string());
    }
}
