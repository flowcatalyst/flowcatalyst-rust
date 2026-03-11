//! Outbox Schema Management
//!
//! Provides SQL to create the outbox_messages table in the consumer's database.
//! Compatible with the fc-outbox-processor.

use sqlx::PgPool;

/// SQL to create the outbox_messages table.
pub const CREATE_OUTBOX_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS outbox_messages (
    id TEXT PRIMARY KEY,
    type TEXT NOT NULL,
    message_group TEXT,
    payload JSONB NOT NULL,
    status INTEGER NOT NULL DEFAULT 0,
    retry_count INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    error_message TEXT,
    client_id TEXT,
    payload_size INTEGER,
    headers JSONB
);

CREATE INDEX IF NOT EXISTS idx_outbox_status_type
    ON outbox_messages(status, type);

CREATE INDEX IF NOT EXISTS idx_outbox_message_group
    ON outbox_messages(message_group)
    WHERE message_group IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_outbox_created_at
    ON outbox_messages(created_at)
    WHERE status = 0;
"#;

/// Initialize the outbox schema in the given database.
///
/// Creates the `outbox_messages` table and indexes if they don't exist.
/// Safe to call multiple times (idempotent).
///
/// # Example
///
/// ```ignore
/// use fc_sdk::outbox::schema::init_outbox_schema;
///
/// let pool = sqlx::PgPool::connect("postgresql://localhost/myapp").await?;
/// init_outbox_schema(&pool).await?;
/// ```
pub async fn init_outbox_schema(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::raw_sql(CREATE_OUTBOX_TABLE_SQL)
        .execute(pool)
        .await?;
    Ok(())
}

/// Initialize the outbox schema with a custom table name.
pub async fn init_outbox_schema_with_table(pool: &PgPool, table_name: &str) -> anyhow::Result<()> {
    let sql = CREATE_OUTBOX_TABLE_SQL.replace("outbox_messages", table_name);
    // Also fix the index names to avoid conflicts
    let sql = sql.replace("idx_outbox_", &format!("idx_{}_", table_name));
    sqlx::raw_sql(&sql).execute(pool).await?;
    Ok(())
}
