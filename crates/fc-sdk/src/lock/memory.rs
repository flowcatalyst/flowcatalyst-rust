//! Process-local lock providers.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::Mutex;

use super::{ensure_positive_ttl, LockError, LockHandle, LockHandleInner, LockProvider};

/// Always-succeeds lock. Use when the underlying work can run concurrently
/// or when you de-dupe by some other means (idempotency keys, partition
/// assignment, etc.).
#[derive(Clone, Default)]
pub struct NoOpLockProvider;

impl NoOpLockProvider {
    pub fn new() -> Self {
        Self
    }
}

struct NoOpHandle;

#[async_trait]
impl LockHandleInner for NoOpHandle {
    async fn release(&mut self) {}
}

#[async_trait]
impl LockProvider for NoOpLockProvider {
    async fn acquire(
        &self,
        _key: &str,
        _ttl: Duration,
    ) -> Result<Option<LockHandle>, LockError> {
        Ok(Some(LockHandle::new(Box::new(NoOpHandle))))
    }
}

/// Process-local mutex. Serialises holders for a given key inside this
/// process. Does NOT survive multiple replicas — use [`super::PgLockProvider`]
/// or [`super::RedisLockProvider`] for that.
#[derive(Clone, Default)]
pub struct MemoryLockProvider {
    held: Arc<Mutex<HashMap<String, Instant>>>,
}

impl MemoryLockProvider {
    pub fn new() -> Self {
        Self::default()
    }
}

struct MemoryHandle {
    key: String,
    expires_at: Instant,
    held: Arc<Mutex<HashMap<String, Instant>>>,
    released: bool,
}

#[async_trait]
impl LockHandleInner for MemoryHandle {
    async fn release(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        let mut guard = self.held.lock().await;
        if let Some(current) = guard.get(&self.key) {
            // Only remove if our own expiry is still current — protects
            // against double-release racing a later acquire.
            if *current == self.expires_at {
                guard.remove(&self.key);
            }
        }
    }
}

#[async_trait]
impl LockProvider for MemoryLockProvider {
    async fn acquire(
        &self,
        key: &str,
        ttl: Duration,
    ) -> Result<Option<LockHandle>, LockError> {
        ensure_positive_ttl(ttl)?;
        let mut guard = self.held.lock().await;
        let now = Instant::now();
        if let Some(existing) = guard.get(key) {
            if *existing > now {
                return Ok(None);
            }
        }
        let expires_at = now
            .checked_add(ttl)
            .ok_or_else(|| LockError::Backend("TTL overflow on Instant".into()))?;
        guard.insert(key.to_string(), expires_at);
        Ok(Some(LockHandle::new(Box::new(MemoryHandle {
            key: key.to_string(),
            expires_at,
            held: self.held.clone(),
            released: false,
        }))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn noop_always_succeeds() {
        let lock = NoOpLockProvider::new();
        let h = lock
            .acquire("k", Duration::from_secs(1))
            .await
            .unwrap()
            .unwrap();
        h.release().await;
    }

    #[tokio::test]
    async fn memory_excludes_concurrent_holders() {
        let lock = MemoryLockProvider::new();
        let h1 = lock
            .acquire("k", Duration::from_secs(30))
            .await
            .unwrap()
            .expect("first acquire");
        let h2 = lock
            .acquire("k", Duration::from_secs(30))
            .await
            .unwrap();
        assert!(h2.is_none(), "second acquire should fail while h1 holds");
        h1.release().await;
        let h3 = lock
            .acquire("k", Duration::from_secs(30))
            .await
            .unwrap();
        assert!(h3.is_some(), "should reacquire after release");
        h3.unwrap().release().await;
    }

    #[tokio::test]
    async fn memory_expires_after_ttl() {
        let lock = MemoryLockProvider::new();
        let _h = lock
            .acquire("k", Duration::from_millis(10))
            .await
            .unwrap()
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        let h2 = lock
            .acquire("k", Duration::from_secs(30))
            .await
            .unwrap();
        assert!(h2.is_some(), "should acquire after previous TTL expires");
    }

    #[tokio::test]
    async fn memory_rejects_zero_ttl() {
        let lock = MemoryLockProvider::new();
        let err = lock.acquire("k", Duration::ZERO).await.unwrap_err();
        assert!(matches!(err, LockError::InvalidTtl));
    }
}
