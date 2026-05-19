//! Redis-backed cache via the workspace `redis` crate.
//!
//! Uses `SET key value PX millis` for writes (atomic with TTL) and `GET` for
//! reads. TTL is enforced by Redis itself, so there's no separate reaper to
//! run — expired keys disappear automatically.

use std::time::Duration;

use async_trait::async_trait;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

use super::{ensure_positive_ttl, Cache, CacheError};

/// Redis-backed cache. Wraps a [`ConnectionManager`] so reconnect handling
/// is automatic — pass one constructed from
/// `ConnectionManager::new(client).await?`.
///
/// Keys are namespaced with an optional prefix to keep multiple SDK
/// consumers from colliding on a shared Redis instance:
/// `format!("{prefix}:{key}")`.
#[derive(Clone)]
pub struct RedisCache {
    conn: ConnectionManager,
    prefix: String,
}

impl RedisCache {
    /// Build a cache with no key prefix.
    pub fn new(conn: ConnectionManager) -> Self {
        Self {
            conn,
            prefix: String::new(),
        }
    }

    /// Build a cache that prepends `prefix:` to every key.
    pub fn with_prefix(conn: ConnectionManager, prefix: impl Into<String>) -> Self {
        Self {
            conn,
            prefix: prefix.into(),
        }
    }

    fn make_key(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            key.to_string()
        } else {
            format!("{}:{}", self.prefix, key)
        }
    }
}

#[async_trait]
impl Cache for RedisCache {
    async fn get_bytes(&self, key: &str) -> Result<Option<Vec<u8>>, CacheError> {
        let mut conn = self.conn.clone();
        let full_key = self.make_key(key);
        let value: Option<Vec<u8>> = conn
            .get(&full_key)
            .await
            .map_err(|e| CacheError::Backend(e.to_string()))?;
        Ok(value)
    }

    async fn set_bytes(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Duration,
    ) -> Result<(), CacheError> {
        ensure_positive_ttl(ttl)?;
        let mut conn = self.conn.clone();
        let full_key = self.make_key(key);
        let ttl_ms: usize = ttl
            .as_millis()
            .try_into()
            .map_err(|_| CacheError::Backend("TTL exceeds Redis maximum".into()))?;
        let _: () = conn
            .pset_ex(&full_key, value, ttl_ms as u64)
            .await
            .map_err(|e| CacheError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), CacheError> {
        let mut conn = self.conn.clone();
        let full_key = self.make_key(key);
        let _: i64 = conn
            .del(&full_key)
            .await
            .map_err(|e| CacheError::Backend(e.to_string()))?;
        Ok(())
    }
}
