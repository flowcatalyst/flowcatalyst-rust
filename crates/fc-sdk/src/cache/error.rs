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

    /// TTL is larger than the backend (or the platform clock) can represent.
    #[error("cache TTL {0:?} is too large for this backend")]
    TtlTooLarge(std::time::Duration),

    /// Backend-level I/O failure (network, query, etc.). Custom [`super::Cache`]
    /// implementations box their native error into this variant.
    #[error("cache backend error: {0}")]
    Backend(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// Stored bytes could not be decoded into the requested type.
    #[error("cache value deserialization failed: {0}")]
    Deserialize(#[source] serde_json::Error),

    /// Caller value could not be JSON-encoded for storage.
    #[error("cache value serialization failed: {0}")]
    Serialize(#[source] serde_json::Error),
}

#[cfg(feature = "cache-postgres")]
impl From<sqlx::Error> for CacheError {
    fn from(e: sqlx::Error) -> Self {
        Self::Backend(Box::new(e))
    }
}

#[cfg(feature = "cache-redis")]
impl From<redis::RedisError> for CacheError {
    fn from(e: redis::RedisError) -> Self {
        Self::Backend(Box::new(e))
    }
}
