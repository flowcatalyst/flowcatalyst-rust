//! Migration helper for the [`PgLockProvider`](super::PgLockProvider) table.
//!
//! Run [`init_lock_schema`] once at startup before the first `acquire`. The
//! create statements are idempotent (`IF NOT EXISTS`).

use sqlx::PgPool;

/// SQL to create the lock table + supporting index. Default table name is
/// `fc_locks`; use [`init_lock_schema_with_table`] to override.
pub const CREATE_LOCK_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS {table} (
    key TEXT PRIMARY KEY,
    holder TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS {table}_expires_at_idx ON {table} (expires_at);
"#;

/// Create the `fc_locks` table with the default name. Safe to run repeatedly.
pub async fn init_lock_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    init_lock_schema_with_table(pool, "fc_locks").await
}

/// Create the lock table with a custom name.
pub async fn init_lock_schema_with_table(
    pool: &PgPool,
    table: &str,
) -> Result<(), sqlx::Error> {
    let sql = CREATE_LOCK_TABLE_SQL.replace("{table}", table);
    for stmt in sql.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        sqlx::query(stmt).execute(pool).await?;
    }
    Ok(())
}
