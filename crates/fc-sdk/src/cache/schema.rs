//! Migration helper for the [`PgCache`](super::PgCache) backing table.
//!
//! Run [`init_cache_schema`] once at startup (or fold the SQL into your
//! application's migration tool) before the first `PgCache::get` /
//! `PgCache::set`. Idempotent — `CREATE TABLE IF NOT EXISTS` and
//! `CREATE INDEX IF NOT EXISTS`.

use sqlx::PgPool;

/// SQL to create the cache table + supporting index. Default table name is
/// `fc_cache`; use [`init_cache_schema_with_table`] to override.
pub const CREATE_CACHE_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS {table} (
    key TEXT PRIMARY KEY,
    value BYTEA NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS {table}_expires_at_idx ON {table} (expires_at);
"#;

/// Create the `fc_cache` table with the default name. Safe to run repeatedly.
pub async fn init_cache_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    init_cache_schema_with_table(pool, "fc_cache").await
}

/// Create the cache table with a custom name (for multi-tenant deployments
/// that need separate cache tables per service).
pub async fn init_cache_schema_with_table(
    pool: &PgPool,
    table: &str,
) -> Result<(), sqlx::Error> {
    let sql = CREATE_CACHE_TABLE_SQL.replace("{table}", table);
    // Split on the empty line so we can run create-table + create-index as
    // two statements (some pg drivers don't allow multi-statement queries).
    for stmt in sql.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        sqlx::query(stmt).execute(pool).await?;
    }
    Ok(())
}
