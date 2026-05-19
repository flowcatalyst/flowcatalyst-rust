//! Cache errors.

use thiserror::Error;

/// Errors that can be returned by a [`super::Cache`] implementation.
///
/// Backends map their native errors into one of these variants. The free
/// helpers ([`super::get`], [`super::set`], [`super::get_or_set`]) add
/// `Serialize` and `Deserialize` variants for the JSON conversion.
#[derive(Debug, Error)]
pub enum CacheError {
    /// TTL was zero or negative. Caches require a positive expiry on every
    /// write — see the module-level documentation for the rationale.
    #[error("cache TTL must be greater than zero")]
    InvalidTtl,

    /// Backend-level I/O failure (network, query, etc.).
    #[error("cache backend error: {0}")]
    Backend(String),

    /// Stored bytes could not be decoded into the requested type.
    #[error("cache value deserialization failed: {0}")]
    Deserialize(String),

    /// Caller value could not be JSON-encoded for storage.
    #[error("cache value serialization failed: {0}")]
    Serialize(String),
}
