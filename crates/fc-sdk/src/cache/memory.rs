//! Process-local in-memory cache backed by `tokio::sync::RwLock<HashMap>`.
//!
//! Suitable for tests, single-pod dev servers, and anywhere durable cross-
//! process state isn't needed. Expired entries are reaped lazily on read —
//! no background sweeper, so memory cost is bounded by the number of
//! distinct keys ever written (until each is read again or
//! [`MemoryCache::reap_expired`] is called).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::RwLock;

use super::{ensure_positive_ttl, Cache, CacheError};

struct Entry {
    value: Vec<u8>,
    expires_at: Instant,
}

/// In-process cache for tests and single-pod deployments.
#[derive(Clone, Default)]
pub struct MemoryCache {
    inner: Arc<RwLock<HashMap<String, Entry>>>,
}

impl MemoryCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Walk the map and drop entries whose TTL has elapsed. The lazy reap on
    /// read covers the common case; call this from a periodic task if you
    /// write keys that are rarely read back.
    pub async fn reap_expired(&self) {
        let now = Instant::now();
        let mut guard = self.inner.write().await;
        guard.retain(|_, entry| entry.expires_at > now);
    }
}

#[async_trait]
impl Cache for MemoryCache {
    async fn get_bytes(&self, key: &str) -> Result<Option<Vec<u8>>, CacheError> {
        let now = Instant::now();
        // Fast path: read lock, return clone if alive.
        {
            let guard = self.inner.read().await;
            if let Some(entry) = guard.get(key) {
                if entry.expires_at > now {
                    return Ok(Some(entry.value.clone()));
                }
            } else {
                return Ok(None);
            }
        }
        // Expired — escalate to a write lock to remove it.
        let mut guard = self.inner.write().await;
        if let Some(entry) = guard.get(key) {
            if entry.expires_at <= now {
                guard.remove(key);
            }
        }
        Ok(None)
    }

    async fn set_bytes(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Duration,
    ) -> Result<(), CacheError> {
        ensure_positive_ttl(ttl)?;
        let expires_at = Instant::now()
            .checked_add(ttl)
            .ok_or_else(|| CacheError::Backend("TTL overflow on Instant".into()))?;
        let mut guard = self.inner.write().await;
        guard.insert(key.to_string(), Entry { value, expires_at });
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), CacheError> {
        let mut guard = self.inner.write().await;
        guard.remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn round_trip() {
        let cache = MemoryCache::new();
        cache
            .set_bytes("k", b"hello".to_vec(), Duration::from_secs(60))
            .await
            .unwrap();
        let v = cache.get_bytes("k").await.unwrap();
        assert_eq!(v.as_deref(), Some(&b"hello"[..]));
    }

    #[tokio::test]
    async fn missing_key_is_none() {
        let cache = MemoryCache::new();
        assert!(cache.get_bytes("nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn delete_removes_entry() {
        let cache = MemoryCache::new();
        cache
            .set_bytes("k", b"v".to_vec(), Duration::from_secs(60))
            .await
            .unwrap();
        cache.delete("k").await.unwrap();
        assert!(cache.get_bytes("k").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn expired_entry_returns_none() {
        let cache = MemoryCache::new();
        cache
            .set_bytes("k", b"v".to_vec(), Duration::from_millis(10))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(cache.get_bytes("k").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn zero_ttl_rejected() {
        let cache = MemoryCache::new();
        let err = cache
            .set_bytes("k", b"v".to_vec(), Duration::ZERO)
            .await
            .unwrap_err();
        assert!(matches!(err, CacheError::InvalidTtl));
    }

    #[tokio::test]
    async fn typed_get_set_round_trip() {
        let cache = MemoryCache::new();
        super::super::set(&cache, "user:1", &"Alice".to_string(), Duration::from_secs(60))
            .await
            .unwrap();
        let v: Option<String> = super::super::get(&cache, "user:1").await.unwrap();
        assert_eq!(v.as_deref(), Some("Alice"));
    }

    #[tokio::test]
    async fn get_or_set_returns_cached_on_hit() {
        let cache = MemoryCache::new();
        super::super::set(&cache, "k", &"cached".to_string(), Duration::from_secs(60))
            .await
            .unwrap();
        let counter = std::sync::Arc::new(std::sync::Mutex::new(0));
        let counter_clone = counter.clone();
        let v: String = super::super::get_or_set(
            &cache,
            "k",
            Duration::from_secs(60),
            move || async move {
                *counter_clone.lock().unwrap() += 1;
                Ok("fresh".to_string())
            },
        )
        .await
        .unwrap();
        assert_eq!(v, "cached");
        assert_eq!(*counter.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn get_or_set_invokes_supplier_on_miss() {
        let cache = MemoryCache::new();
        let v: String = super::super::get_or_set(
            &cache,
            "k",
            Duration::from_secs(60),
            || async { Ok("fresh".to_string()) },
        )
        .await
        .unwrap();
        assert_eq!(v, "fresh");
        let stored: Option<String> = super::super::get(&cache, "k").await.unwrap();
        assert_eq!(stored.as_deref(), Some("fresh"));
    }

    #[tokio::test]
    async fn reap_expired_drops_stale_entries() {
        let cache = MemoryCache::new();
        cache
            .set_bytes("a", b"1".to_vec(), Duration::from_millis(5))
            .await
            .unwrap();
        cache
            .set_bytes("b", b"2".to_vec(), Duration::from_secs(60))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        cache.reap_expired().await;
        let guard = cache.inner.read().await;
        assert!(!guard.contains_key("a"));
        assert!(guard.contains_key("b"));
    }
}
