//! `fc-migrate <database-url>`: apply every SQL migration the platform
//! applies (`MigrationProfile::Production`), the way the application does.
//!
//! Used by `scripts/sqlx-prepare.sh` to build the schema the checked queries
//! are prepared against. The platform's code migrations (036, the
//! scheduled-job cron rewrite) rewrite data, not schema, so they are not run.

use std::env;
use std::process::ExitCode;

use fc_migrations::{run_migrations_with, MigrationProfile};
use sqlx::postgres::PgPool;

#[tokio::main]
async fn main() -> ExitCode {
    let Some(url) = env::args().nth(1) else {
        eprintln!("usage: fc-migrate <postgresql-url>");
        return ExitCode::from(2);
    };
    let pool = match PgPool::connect(&url).await {
        Ok(pool) => pool,
        Err(e) => {
            eprintln!("fc-migrate: cannot connect: {e}");
            return ExitCode::FAILURE;
        }
    };
    match run_migrations_with(&pool, MigrationProfile::Production, &[]).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fc-migrate: {e}");
            ExitCode::FAILURE
        }
    }
}
