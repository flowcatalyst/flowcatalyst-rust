//! Redis-backed `RateLimitStore`.
//!
//! Fixed-window counter: `INCR fc:rl:{bucket}:{key}:{window_index}`,
//! `EXPIRE` to the window length when the counter first transitions
//! from 0→1, reject when the counter exceeds the policy limit.
//!
//! `window_index = floor(now_secs / window_secs)` so a fresh key is
//! created at the start of each window and aged out automatically by
//! the TTL — no reaper needed. The downside vs. a sliding window is a
//! 2× spike right at the window boundary, which is acceptable here:
//! the in-memory `governor` middleware still smooths short bursts at
//! the instance level, and this store's job is just to keep
//! cluster-wide volume from running away.

use std::time::Duration;

use async_trait::async_trait;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use tracing::warn;

use super::{Bucket, RateLimitDecision, RateLimitError, RateLimitPolicy, RateLimitStore};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

pub struct RedisRateLimitStore {
    conn: ConnectionManager,
}

impl RedisRateLimitStore {
    /// Connect, run a `PING`, and return the live store. Caller is
    /// expected to fall back to Postgres on `Err` — never panics.
    pub async fn connect(url: &str) -> Result<Self, RateLimitError> {
        let client = ::redis::Client::open(url)
            .map_err(|e| RateLimitError::Backend(format!("invalid redis url: {}", e)))?;

        let mgr = tokio::time::timeout(CONNECT_TIMEOUT, ConnectionManager::new(client))
            .await
            .map_err(|_| RateLimitError::Backend("redis connect timed out".into()))?
            .map_err(|e| RateLimitError::Backend(format!("redis connect: {}", e)))?;

        // PING — confirm the connection is actually live before we
        // declare Redis "available" and skip the Postgres fallback.
        let mut conn = mgr.clone();
        let pong: String = tokio::time::timeout(CONNECT_TIMEOUT, ::redis::cmd("PING").query_async(&mut conn))
            .await
            .map_err(|_| RateLimitError::Backend("redis PING timed out".into()))?
            .map_err(|e| RateLimitError::Backend(format!("redis PING: {}", e)))?;
        if pong != "PONG" {
            return Err(RateLimitError::Backend(format!(
                "unexpected redis PING response: {}",
                pong
            )));
        }

        Ok(Self { conn: mgr })
    }

    fn make_key(bucket: Bucket, key: &str, window_index: u64) -> String {
        format!("fc:rl:{}:{}:{}", bucket.as_str(), key, window_index)
    }
}

#[async_trait]
impl RateLimitStore for RedisRateLimitStore {
    async fn check_and_record(
        &self,
        bucket: Bucket,
        key: &str,
        policy: RateLimitPolicy,
    ) -> Result<RateLimitDecision, RateLimitError> {
        let window_secs = policy.window.as_secs().max(1);
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let window_index = now_secs / window_secs;
        let redis_key = Self::make_key(bucket, key, window_index);

        let mut conn = self.conn.clone();

        // INCR returns the new count. On the 0→1 transition we set a TTL
        // matching the window length so the key disappears once the
        // window ends. Subsequent INCRs in the same window keep the
        // existing TTL.
        let new_count: i64 = conn
            .incr(&redis_key, 1_i64)
            .await
            .map_err(|e| RateLimitError::Backend(e.to_string()))?;

        if new_count == 1 {
            // Expire is best-effort — a TTL miss just means the key
            // lives until the next call's EXPIRE catches it (worst case
            // a few extra requests counted against the next window).
            // Log at debug, not warn — Redis EXPIRE failures are
            // operational noise, not security events.
            if let Err(e) = conn.expire::<_, ()>(&redis_key, window_secs as i64).await {
                warn!(error = %e, key = %redis_key, "redis EXPIRE failed; TTL may be missing on this window");
            }
        }

        if (new_count as u64) > policy.limit as u64 {
            // Retry-after = time until the current window rolls over.
            // Within ±window_secs of accurate; good enough for a
            // Retry-After header.
            let elapsed_in_window = now_secs % window_secs;
            let retry_after_secs = window_secs.saturating_sub(elapsed_in_window).max(1);
            return Ok(RateLimitDecision::Reject {
                retry_after_secs: retry_after_secs.min(u32::MAX as u64) as u32,
            });
        }

        Ok(RateLimitDecision::Allow)
    }

    // prune() is a no-op: Redis TTLs auto-expire fixed-window keys.
}
