//! Distributed lock
//!
//! Pluggable lock contract mirroring the TypeScript SDK's `LockProvider`.
//! Used by consumer apps to serialise work across replicas — typically
//! `concurrent: false` scheduled jobs that may fire on more than one pod.
//!
//! Every backend takes a **required** `ttl: Duration` on acquire — the lock
//! self-expires after the deadline so a crashed holder doesn't permanently
//! poison the key. Pick a TTL longer than your expected critical section
//! plus some headroom; the lock can be released early with
//! [`LockHandle::release`].
//!
//! # Backends
//!
//! - [`NoOpLockProvider`] — every acquire succeeds. Use when the job is
//!   `concurrent: true`, you only run one consumer pod, or you de-dupe via
//!   idempotency keys.
//! - [`MemoryLockProvider`] — process-local mutex. Useful for single-Node
//!   processes where you just want to serialise within one replica.
//! - [`PgLockProvider`] (feature `lock-postgres`) — table-based lock using
//!   conditional upsert; survives crashes via TTL. Ships
//!   [`init_lock_schema`].
//! - [`RedisLockProvider`] (feature `lock-redis`) — `SET NX PX` with a Lua
//!   release script that only deletes the lock if our token still owns it.
//!
//! # Example
//!
//! ```ignore
//! use fc_sdk::lock::{LockProvider, MemoryLockProvider};
//! use std::sync::Arc;
//! use std::time::Duration;
//!
//! let lock: Arc<dyn LockProvider> = Arc::new(MemoryLockProvider::new());
//!
//! match lock.acquire("orders:dispatch", Duration::from_secs(30)).await? {
//!     Some(handle) => {
//!         // critical section …
//!         handle.release().await;
//!     }
//!     None => {
//!         // another holder owns it — skip this firing
//!     }
//! }
//! ```

use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;

mod memory;

#[cfg(feature = "lock-postgres")]
mod pg;
#[cfg(feature = "lock-postgres")]
mod schema;
#[cfg(feature = "lock-redis")]
mod redis;

pub use memory::{MemoryLockProvider, NoOpLockProvider};

#[cfg(feature = "lock-postgres")]
pub use pg::PgLockProvider;
#[cfg(feature = "lock-postgres")]
pub use schema::{init_lock_schema, init_lock_schema_with_table, CREATE_LOCK_TABLE_SQL};
#[cfg(feature = "lock-redis")]
pub use self::redis::RedisLockProvider;

/// Errors from a [`LockProvider`] implementation. Acquire-result-of-`None`
/// (i.e. lock contended) is NOT an error — only backend faults are.
#[derive(Debug, Error)]
pub enum LockError {
    /// TTL was zero or negative.
    #[error("lock TTL must be greater than zero")]
    InvalidTtl,
    /// Backend-level I/O failure (network, query, etc.).
    #[error("lock backend error: {0}")]
    Backend(String),
}

/// Pluggable distributed lock contract. Mirrors the TypeScript SDK's
/// `LockProvider` interface.
///
/// `acquire` is non-blocking: it returns `Ok(None)` immediately when the key
/// is held by another holder. The caller decides whether to retry, skip, or
/// fail. `ttl` bounds how long a crashed holder can keep the lock for.
#[async_trait]
pub trait LockProvider: Send + Sync {
    /// Try to acquire `key` for at most `ttl`. Returns `Ok(Some(handle))` on
    /// success, `Ok(None)` if another holder owns it, and `Err` only on
    /// backend faults.
    async fn acquire(
        &self,
        key: &str,
        ttl: Duration,
    ) -> Result<Option<LockHandle>, LockError>;
}

/// Handle returned by a successful [`LockProvider::acquire`]. Drop will NOT
/// auto-release — callers should always call [`LockHandle::release`] when
/// they're done with the critical section. (Drop can't run async, so
/// best-effort cleanup is the backend's responsibility via TTL expiry.)
pub struct LockHandle {
    inner: Box<dyn LockHandleInner>,
}

impl std::fmt::Debug for LockHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LockHandle").finish_non_exhaustive()
    }
}

impl LockHandle {
    pub(crate) fn new(inner: Box<dyn LockHandleInner>) -> Self {
        Self { inner }
    }

    /// Release the lock. Idempotent — safe to call multiple times, though
    /// only the first call does work.
    pub async fn release(mut self) {
        self.inner.release().await;
    }
}

/// Internal trait implemented by each backend's concrete handle. Boxed
/// inside [`LockHandle`] so consumers see one concrete type.
#[async_trait]
pub(crate) trait LockHandleInner: Send + Sync {
    async fn release(&mut self);
}

pub(crate) fn ensure_positive_ttl(ttl: Duration) -> Result<(), LockError> {
    if ttl.as_millis() == 0 {
        return Err(LockError::InvalidTtl);
    }
    Ok(())
}
