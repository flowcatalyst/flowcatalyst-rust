//! Error type for the message router.
//!
//! The router's public functions return [`crate::Result`]
//! (`Result<T, RouterError>`). In practice the router resolves nearly
//! everything as ack/nack at the message layer and logs-and-swallows
//! operational failures, so this enum is deliberately small: one explicit
//! control-flow signal plus the two transport/codec conversions that `?` can
//! produce. Config-sync carries its own typed
//! [`crate::config_sync::ConfigSyncError`].
//!
//! (Earlier this enum had a spread of stringly-typed variants —
//! `Pool(String)`, `Queue(String)`, `PoolNotFound(String)`, … — none of which
//! were ever constructed. They were a transliteration artifact and have been
//! removed; reintroduce a *typed* variant if a real failure path needs one.)

use thiserror::Error;

#[derive(Error, Debug)]
pub enum RouterError {
    /// The manager is shutting down, so the batch was rejected (its messages
    /// nacked) rather than routed. Surfaced to the consumer poll loop, which
    /// logs it and stops polling.
    #[error("Shutdown in progress")]
    ShutdownInProgress,

    /// An outbound HTTP request failed at the transport level. Auto-converted
    /// from `reqwest::Error` via `?`.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// JSON (de)serialisation failed. Auto-converted from `serde_json::Error`
    /// via `?`.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// A [`crate::manager::ConsumerFactory`] failed to build a consumer for
    /// a queue — unrecognised URI scheme (item 1: `fc_queue::scheme`), or
    /// the backend-specific connect/provisioning step itself errored (NATS
    /// stream/consumer provisioning, the Postgres schema/pool bootstrap).
    #[error("Consumer factory error: {0}")]
    Consumer(String),
}
