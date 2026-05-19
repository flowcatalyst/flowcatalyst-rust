//! Postgres-backed cache via `sqlx`.
//!
//! Stores values as `BYTEA` in a `fc_cache` table (see [`super::schema`] for
//! the migration helper). Reads filter on `expires_at > NOW()` so an expired
//! row is invisible even before it's reaped; writes upsert on the primary
//! key so callers can refresh the TTL by writing again.
//!
//! Stale rows are reaped lazily by [`PgCache::reap_expired`]; call it from a
//! periodic task if you write keys that are rarely read back.

use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use sqlx::PgPool;

use super::{ensure_positive_ttl, Cache, CacheError};

/// `sqlx`-backed cache. Default table is `fc_cache`; override with
/// [`PgCache::with_table`].
#[derive(Clone)]
pub struct PgCache {
    pool: PgPool,
    table: String,
}

impl PgCache {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            table: "fc_cache".into(),
        }
    }

    pub fn with_table(pool: PgPool, table: impl Into<String>) -> Self {
        Self {
            pool,
            table: table.into(),
        }
    }

    /// Delete rows whose TTL has elapsed. Returns the number of rows removed.
    /// Cheap thanks to the index on `expires_at`; safe to call repeatedly.
    pub async fn reap_expired(&self) -> Result<u64, CacheError> {
        let sql = format!("DELETE FROM {} WHERE expires_at <= NOW()", self.table);
        let result = sqlx::query(&sql)
            .execute(&self.pool)
            .await
            .map_err(|e| CacheError::Backend(e.to_string()))?;
        Ok(result.rows_affected())
    }
}

#[async_trait]
impl Cache for PgCache {
    async fn get_bytes(&self, key: &str) -> Result<Option<Vec<u8>>, CacheError> {
        let sql = format!(
            "SELECT value FROM {} WHERE key = $1 AND expires_at > NOW()",
            self.table
        );
        let row: Option<(Vec<u8>,)> = sqlx::query_as(&sql)
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| CacheError::Backend(e.to_string()))?;
        Ok(row.map(|(v,)| v))
    }

    async fn set_bytes(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Duration,
    ) -> Result<(), CacheError> {
        ensure_positive_ttl(ttl)?;
        let expires_at = Utc::now()
            + chrono::Duration::from_std(ttl)
                .map_err(|e| CacheError::Backend(format!("TTL too large: {}", e)))?;

        let sql = format!(
            "INSERT INTO {} (key, value, expires_at) VALUES ($1, $2, $3) \
             ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, expires_at = EXCLUDED.expires_at",
            self.table
        );
        sqlx::query(&sql)
            .bind(key)
            .bind(&value)
            .bind(expires_at)
            .execute(&self.pool)
            .await
            .map_err(|e| CacheError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), CacheError> {
        let sql = format!("DELETE FROM {} WHERE key = $1", self.table);
        sqlx::query(&sql)
            .bind(key)
            .execute(&self.pool)
            .await
            .map_err(|e| CacheError::Backend(e.to_string()))?;
        Ok(())
    }
}
