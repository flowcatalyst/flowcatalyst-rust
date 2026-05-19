//! Cache
//!
//! Pluggable key-value cache with **required TTL** on every write. The trait
//! is byte-oriented so it stays dyn-compatible (the SDK uses
//! `Arc<dyn Cache>` for DI); typed access goes through the free helpers
//! [`get`], [`set`], and [`get_or_set`], which JSON-encode/decode values via
//! `serde`.
//!
//! TTL is non-optional on purpose — caches without expiry silently grow into
//! a memory leak in long-running services. Every write *must* pick a
//! deadline. Use a long `Duration` if you really want "rarely expires".
//!
//! # Backends
//!
//! - [`MemoryCache`] — process-local, default for tests and single-pod dev.
//!   No new dependencies; uses `tokio::sync::RwLock<HashMap>`.
//! - [`PgCache`] (feature `cache-postgres`) — sqlx-backed table cache. Ships
//!   [`init_cache_schema`] to create the `fc_cache` table.
//! - [`RedisCache`] (feature `cache-redis`) — uses the workspace `redis`
//!   crate with TTL via `PEX`.
//!
//! # Example
//!
//! ```ignore
//! use fc_sdk::cache::{Cache, MemoryCache};
//! use std::sync::Arc;
//! use std::time::Duration;
//!
//! let cache: Arc<dyn Cache> = Arc::new(MemoryCache::new());
//!
//! fc_sdk::cache::set(&*cache, "user:123", &"Alice", Duration::from_secs(60)).await?;
//! let value: Option<String> = fc_sdk::cache::get(&*cache, "user:123").await?;
//! ```

use std::time::Duration;

use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};

mod error;
mod memory;

#[cfg(feature = "cache-postgres")]
mod pg;
#[cfg(feature = "cache-postgres")]
mod schema;
#[cfg(feature = "cache-redis")]
mod redis;

pub use error::CacheError;
pub use memory::MemoryCache;

#[cfg(feature = "cache-postgres")]
pub use pg::PgCache;
#[cfg(feature = "cache-postgres")]
pub use schema::{init_cache_schema, init_cache_schema_with_table, CREATE_CACHE_TABLE_SQL};
#[cfg(feature = "cache-redis")]
pub use self::redis::RedisCache;

/// Pluggable cache contract. Implementations store opaque bytes; typed access
/// is provided by the [`get`], [`set`], and [`get_or_set`] free helpers.
///
/// **TTL is required** on every write — a cache entry without an expiry is
/// almost always a bug in long-running services. If you genuinely want
/// near-permanent storage, pass a very long `Duration`.
#[async_trait]
pub trait Cache: Send + Sync {
    /// Read the raw bytes for `key`. Returns `Ok(None)` for a miss OR for an
    /// entry whose TTL has elapsed (expired entries are treated as missing
    /// regardless of whether the backend has cleaned them up yet).
    async fn get_bytes(&self, key: &str) -> Result<Option<Vec<u8>>, CacheError>;

    /// Write `value` for `key`, expiring after `ttl`. Overwrites any existing
    /// value. Implementations must reject zero / negative TTLs by returning
    /// [`CacheError::InvalidTtl`].
    async fn set_bytes(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Duration,
    ) -> Result<(), CacheError>;

    /// Remove `key`. Returns `Ok(())` whether or not the key existed.
    async fn delete(&self, key: &str) -> Result<(), CacheError>;
}

/// Typed read: JSON-decodes the bytes returned by [`Cache::get_bytes`].
///
/// Returns `Ok(None)` for a miss / expired entry. Returns
/// [`CacheError::Deserialize`] if the stored bytes don't decode into `T`.
pub async fn get<T: DeserializeOwned>(
    cache: &(dyn Cache + '_),
    key: &str,
) -> Result<Option<T>, CacheError> {
    match cache.get_bytes(key).await? {
        Some(bytes) => match serde_json::from_slice::<T>(&bytes) {
            Ok(v) => Ok(Some(v)),
            Err(e) => Err(CacheError::Deserialize(e.to_string())),
        },
        None => Ok(None),
    }
}

/// Typed write: JSON-encodes `value` then forwards to [`Cache::set_bytes`].
pub async fn set<T: Serialize + Sync>(
    cache: &(dyn Cache + '_),
    key: &str,
    value: &T,
    ttl: Duration,
) -> Result<(), CacheError> {
    let bytes =
        serde_json::to_vec(value).map_err(|e| CacheError::Serialize(e.to_string()))?;
    cache.set_bytes(key, bytes, ttl).await
}

/// Read-through helper. Returns the cached value if present; otherwise calls
/// `supplier`, caches the result with `ttl`, and returns it.
///
/// Not atomic across replicas: two callers racing on the same key may both
/// invoke `supplier` (the loser's write overwrites the winner's). If you
/// need exactly-once supplier execution, layer a [`crate::lock::LockProvider`]
/// around the call.
pub async fn get_or_set<T, F, Fut>(
    cache: &(dyn Cache + '_),
    key: &str,
    ttl: Duration,
    supplier: F,
) -> Result<T, CacheError>
where
    T: Serialize + DeserializeOwned + Sync,
    F: FnOnce() -> Fut + Send,
    Fut: std::future::Future<Output = Result<T, CacheError>> + Send,
{
    if let Some(v) = get::<T>(cache, key).await? {
        return Ok(v);
    }
    let value = supplier().await?;
    set(cache, key, &value, ttl).await?;
    Ok(value)
}

/// Guard against zero / negative TTLs at the trait boundary so every backend
/// gets the same error shape.
pub(crate) fn ensure_positive_ttl(ttl: Duration) -> Result<(), CacheError> {
    if ttl.as_millis() == 0 {
        return Err(CacheError::InvalidTtl);
    }
    Ok(())
}
