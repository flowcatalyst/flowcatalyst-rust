//! Postgres-backed `RateLimitStore`.
//!
//! Appends one row to `iam_rate_limit_events` per attempt and counts
//! rows in the window for the (bucket, key). Modeled on the same
//! sliding-window-counter pattern as `auth::login_backoff` reading from
//! `iam_login_attempts`.
//!
//! The combined record+count happens in a single `INSERT … RETURNING
//! (SELECT COUNT(*) …)` statement so the read sees its own write — no
//! race between two replicas where both insert, both count "below
//! limit," and both let the request through.

use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use sqlx::PgPool;

use super::{Bucket, RateLimitDecision, RateLimitError, RateLimitPolicy, RateLimitStore};

pub struct PostgresRateLimitStore {
    pool: PgPool,
}

impl PostgresRateLimitStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RateLimitStore for PostgresRateLimitStore {
    async fn check_and_record(
        &self,
        bucket: Bucket,
        key: &str,
        policy: RateLimitPolicy,
    ) -> Result<RateLimitDecision, RateLimitError> {
        let window_start = Utc::now()
            - chrono::Duration::from_std(policy.window).unwrap_or(chrono::Duration::zero());

        // Atomic insert + count-within-window. The CTE inserts unconditionally
        // (every attempt counts toward the window total, including the one
        // we're checking), then the SELECT returns the new total. One round
        // trip; no read-modify-write race. A data-modifying CTE's row is not
        // visible to the rest of its own statement (the statement's snapshot
        // predates it), so the row just inserted is added explicitly —
        // without it the store allowed `limit + 1`, unlike Redis's INCR.
        let count: i64 = sqlx::query_scalar(
            r#"
            WITH ins AS (
                INSERT INTO iam_rate_limit_events (bucket, key, occurred_at)
                VALUES ($1, $2, NOW())
                RETURNING 1
            )
            SELECT (SELECT COUNT(*) FROM ins) + COUNT(*)
            FROM iam_rate_limit_events
            WHERE bucket = $1
              AND key = $2
              AND occurred_at > $3
            "#,
        )
        .bind(bucket.as_str())
        .bind(key)
        .bind(window_start)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| RateLimitError::Backend(e.to_string()))?;

        if (count as u64) > policy.limit as u64 {
            // Best-effort retry-after estimate: time until the oldest event
            // in the window falls outside it. Caps at the full window so
            // misconfigured policies don't return absurd numbers.
            let oldest: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
                "SELECT MIN(occurred_at) FROM iam_rate_limit_events \
                 WHERE bucket = $1 AND key = $2 AND occurred_at > $3",
            )
            .bind(bucket.as_str())
            .bind(key)
            .bind(window_start)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| RateLimitError::Backend(e.to_string()))?;

            let retry_after_secs = oldest
                .map(|o| {
                    let elapsed = (Utc::now() - o).num_seconds().max(0) as u64;
                    policy.window.as_secs().saturating_sub(elapsed).max(1)
                })
                .unwrap_or_else(|| policy.window.as_secs().max(1));

            return Ok(RateLimitDecision::Reject {
                retry_after_secs: retry_after_secs.min(u32::MAX as u64) as u32,
            });
        }

        Ok(RateLimitDecision::Allow)
    }

    async fn prune(&self, older_than: Duration) -> Result<u64, RateLimitError> {
        let cutoff =
            Utc::now() - chrono::Duration::from_std(older_than).unwrap_or(chrono::Duration::zero());
        let res = sqlx::query("DELETE FROM iam_rate_limit_events WHERE occurred_at < $1")
            .bind(cutoff)
            .execute(&self.pool)
            .await
            .map_err(|e| RateLimitError::Backend(e.to_string()))?;
        Ok(res.rows_affected())
    }
}
