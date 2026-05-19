//! Redis-backed distributed lock.
//!
//! `SET NX PX <ttl_ms>` for acquire (atomic with TTL), Lua check-and-delete
//! for release so we only delete locks whose token we still own. This
//! protects against a stale releaser stomping a lock that's already been
//! reclaimed by someone else after a TTL expiry.

use std::time::Duration;

use async_trait::async_trait;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use uuid::Uuid;

use super::{ensure_positive_ttl, LockError, LockHandle, LockHandleInner, LockProvider};

const RELEASE_SCRIPT: &str = r#"
if redis.call("GET", KEYS[1]) == ARGV[1] then
    return redis.call("DEL", KEYS[1])
else
    return 0
end
"#;

/// Redis-backed lock provider. Pass a [`ConnectionManager`] so reconnect
/// handling is automatic.
#[derive(Clone)]
pub struct RedisLockProvider {
    conn: ConnectionManager,
    prefix: String,
}

impl RedisLockProvider {
    pub fn new(conn: ConnectionManager) -> Self {
        Self {
            conn,
            prefix: "fc:lock".into(),
        }
    }

    pub fn with_prefix(conn: ConnectionManager, prefix: impl Into<String>) -> Self {
        Self {
            conn,
            prefix: prefix.into(),
        }
    }

    fn make_key(&self, key: &str) -> String {
        format!("{}:{}", self.prefix, key)
    }
}

struct RedisLockHandle {
    conn: ConnectionManager,
    full_key: String,
    token: String,
    released: bool,
}

#[async_trait]
impl LockHandleInner for RedisLockHandle {
    async fn release(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        let mut conn = self.conn.clone();
        let res: redis::RedisResult<i64> = redis::Script::new(RELEASE_SCRIPT)
            .key(&self.full_key)
            .arg(&self.token)
            .invoke_async(&mut conn)
            .await;
        if let Err(e) = res {
            tracing::warn!(
                error = %e,
                key = %self.full_key,
                "Failed to release RedisLockProvider lock; relying on TTL expiry",
            );
        }
    }
}

#[async_trait]
impl LockProvider for RedisLockProvider {
    async fn acquire(
        &self,
        key: &str,
        ttl: Duration,
    ) -> Result<Option<LockHandle>, LockError> {
        ensure_positive_ttl(ttl)?;
        let mut conn = self.conn.clone();
        let full_key = self.make_key(key);
        let token = Uuid::new_v4().to_string();
        let ttl_ms: u64 = ttl
            .as_millis()
            .try_into()
            .map_err(|_| LockError::Backend("TTL exceeds Redis maximum".into()))?;

        // `set_options` with NX + PX: returns "OK" on success, nil on collision.
        let opts = redis::SetOptions::default()
            .conditional_set(redis::ExistenceCheck::NX)
            .with_expiration(redis::SetExpiry::PX(ttl_ms));
        let result: redis::RedisResult<Option<String>> =
            conn.set_options(&full_key, &token, opts).await;
        match result {
            Ok(Some(_ok)) => Ok(Some(LockHandle::new(Box::new(RedisLockHandle {
                conn: self.conn.clone(),
                full_key,
                token,
                released: false,
            })))),
            Ok(None) => Ok(None),
            Err(e) => Err(LockError::Backend(e.to_string())),
        }
    }
}
