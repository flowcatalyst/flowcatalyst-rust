//! Postgres-backed distributed lock via a `fc_locks` table.
//!
//! `pg_try_advisory_lock` would be faster, but advisory locks have no TTL —
//! a crashed holder keeps the lock until its session ends. With table-based
//! locks the TTL is explicit and enforced by the upsert's `WHERE` clause:
//! another holder can reclaim an expired row.
//!
//! Acquire is a single `INSERT … ON CONFLICT … DO UPDATE … WHERE … RETURNING`
//! statement — atomic in Postgres, so no race window between checking and
//! taking.

use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use super::{ensure_positive_ttl, LockError, LockHandle, LockHandleInner, LockProvider};

/// Table-based lock provider for Postgres.
///
/// Each acquire mints a unique holder token (random UUID) so `release` only
/// deletes locks the caller actually owns — protects against accidental
/// release of a lock that has since been reclaimed by another holder.
#[derive(Clone)]
pub struct PgLockProvider {
    pool: PgPool,
    table: String,
}

impl PgLockProvider {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            table: "fc_locks".into(),
        }
    }

    pub fn with_table(pool: PgPool, table: impl Into<String>) -> Self {
        Self {
            pool,
            table: table.into(),
        }
    }

    /// Delete rows whose TTL has elapsed without being released. Returns the
    /// number of rows removed. Optional — acquire reclaims expired rows
    /// implicitly via the upsert.
    pub async fn reap_expired(&self) -> Result<u64, LockError> {
        let sql = format!("DELETE FROM {} WHERE expires_at <= NOW()", self.table);
        let result = sqlx::query(&sql)
            .execute(&self.pool)
            .await
            .map_err(|e| LockError::Backend(e.to_string()))?;
        Ok(result.rows_affected())
    }
}

struct PgLockHandle {
    pool: PgPool,
    table: String,
    key: String,
    holder: String,
    released: bool,
}

#[async_trait]
impl LockHandleInner for PgLockHandle {
    async fn release(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        let sql = format!("DELETE FROM {} WHERE key = $1 AND holder = $2", self.table);
        // Best-effort — log on failure, never panic on a release path.
        if let Err(e) = sqlx::query(&sql)
            .bind(&self.key)
            .bind(&self.holder)
            .execute(&self.pool)
            .await
        {
            tracing::warn!(
                error = %e,
                key = %self.key,
                "Failed to release PgLockProvider lock; relying on TTL expiry",
            );
        }
    }
}

#[async_trait]
impl LockProvider for PgLockProvider {
    async fn acquire(
        &self,
        key: &str,
        ttl: Duration,
    ) -> Result<Option<LockHandle>, LockError> {
        ensure_positive_ttl(ttl)?;
        let holder = Uuid::new_v4().to_string();
        let expires_at = Utc::now()
            + chrono::Duration::from_std(ttl)
                .map_err(|e| LockError::Backend(format!("TTL too large: {}", e)))?;

        // Upsert with WHERE so we only displace an expired holder. RETURNING
        // returns our holder iff we actually inserted or updated; the no-op
        // case (existing non-expired row) returns nothing.
        let sql = format!(
            "INSERT INTO {table} (key, holder, expires_at) VALUES ($1, $2, $3) \
             ON CONFLICT (key) DO UPDATE \
                SET holder = EXCLUDED.holder, expires_at = EXCLUDED.expires_at \
                WHERE {table}.expires_at <= NOW() \
             RETURNING holder",
            table = self.table
        );

        let result: Option<(String,)> = sqlx::query_as(&sql)
            .bind(key)
            .bind(&holder)
            .bind(expires_at)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| LockError::Backend(e.to_string()))?;

        match result {
            Some((winner,)) if winner == holder => Ok(Some(LockHandle::new(Box::new(
                PgLockHandle {
                    pool: self.pool.clone(),
                    table: self.table.clone(),
                    key: key.to_string(),
                    holder,
                    released: false,
                },
            )))),
            // RETURNING gave nothing OR a different holder — we lost.
            _ => Ok(None),
        }
    }
}
