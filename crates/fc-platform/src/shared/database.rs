//! PostgreSQL Database Connection
//!
//! Provides SeaORM DatabaseConnection setup with connection pooling
//! and SQL migration runner.

use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr};
use std::time::Duration;
use tracing::info;

/// Create a new SeaORM DatabaseConnection with connection pooling.
///
/// # Arguments
/// * `database_url` - PostgreSQL connection URL (e.g., `postgresql://user:pass@host:5432/db`)
///
/// # Environment-configurable pool settings
/// * `FC_DB_MAX_CONNECTIONS` - Maximum pool connections (default: 10)
/// * `FC_DB_MIN_CONNECTIONS` - Minimum idle connections (default: 2)
/// * `FC_DB_CONNECT_TIMEOUT_SECS` - Connection timeout in seconds (default: 10)
/// * `FC_DB_IDLE_TIMEOUT_SECS` - Idle connection timeout in seconds (default: 300)
/// * `FC_DB_MAX_LIFETIME_SECS` - Max connection lifetime in seconds (default: 1800)
/// * `FC_DB_SQLX_LOGGING` - Enable sqlx query logging (default: false)
pub async fn create_connection(database_url: &str) -> Result<DatabaseConnection, DbErr> {
    let max_connections: u32 = std::env::var("FC_DB_MAX_CONNECTIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let min_connections: u32 = std::env::var("FC_DB_MIN_CONNECTIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);
    let connect_timeout: u64 = std::env::var("FC_DB_CONNECT_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let idle_timeout: u64 = std::env::var("FC_DB_IDLE_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    let max_lifetime: u64 = std::env::var("FC_DB_MAX_LIFETIME_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1800);
    let sqlx_logging: bool = std::env::var("FC_DB_SQLX_LOGGING")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(false);

    let mut opts = ConnectOptions::new(database_url);
    opts.max_connections(max_connections)
        .min_connections(min_connections)
        .connect_timeout(Duration::from_secs(connect_timeout))
        .idle_timeout(Duration::from_secs(idle_timeout))
        .max_lifetime(Duration::from_secs(max_lifetime))
        .sqlx_logging(sqlx_logging);

    info!(
        max_connections,
        min_connections,
        "Connecting to PostgreSQL database"
    );

    let db = Database::connect(opts).await?;
    info!("PostgreSQL database connection established");
    Ok(db)
}

/// Run all SQL migrations from the migrations/ directory.
///
/// Uses `CREATE TABLE IF NOT EXISTS` / `CREATE INDEX IF NOT EXISTS` so migrations
/// are safe to run against an existing TypeScript-created database.
pub async fn run_migrations(db: &DatabaseConnection) -> Result<(), DbErr> {
    use sea_orm::ConnectionTrait;

    info!("Running database migrations...");

    let migration_files = [
        include_str!("../../../../migrations/001_tenant_tables.sql"),
        include_str!("../../../../migrations/002_iam_tables.sql"),
        include_str!("../../../../migrations/003_application_tables.sql"),
        include_str!("../../../../migrations/004_messaging_tables.sql"),
        include_str!("../../../../migrations/005_outbox_tables.sql"),
        include_str!("../../../../migrations/006_audit_tables.sql"),
        include_str!("../../../../migrations/007_oauth_tables.sql"),
        include_str!("../../../../migrations/008_auth_tracking_tables.sql"),
        include_str!("../../../../migrations/009_p0_alignment.sql"),
        include_str!("../../../../migrations/010_auth_state_tables.sql"),
        include_str!("../../../../migrations/011_dispatch_job_tables.sql"),
    ];

    for (i, sql) in migration_files.iter().enumerate() {
        // Split on semicolons and execute each statement individually
        for statement in sql.split(';') {
            // Strip comment-only lines, then check if any SQL remains
            let cleaned: String = statement
                .lines()
                .filter(|line| !line.trim_start().starts_with("--"))
                .collect::<Vec<_>>()
                .join("\n");
            let trimmed = cleaned.trim();
            if trimmed.is_empty() {
                continue;
            }
            db.execute(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                trimmed.to_string(),
            ))
            .await?;
        }
        info!("Migration {} applied successfully", i + 1);
    }

    info!("All database migrations completed");
    Ok(())
}
