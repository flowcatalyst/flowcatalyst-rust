//! `fc-dev fresh` — drop the public schema and rebuild it from migrations.
//!
//! Wipes EVERYTHING in the public schema (DDL + data), then:
//!
//!   1. Re-runs every migration to recreate the schema.
//!   2. Re-seeds built-in roles, the `platform` application, default
//!      processes, and a bootstrap admin user.
//!
//! After this runs `fc-dev` (start) can be launched immediately and you
//! can sign in with `admin@flowcatalyst.local` / `DevPassword123!`
//! (override with `--admin-email` / `--admin-password`).
//!
//! Why drop the schema rather than truncate a table allowlist? The
//! allowlist approach used to be the implementation and it silently
//! drifted: when a new migration introduced the `app_*` tables they were
//! left untouched by `fresh`, so applications survived a "reset." A full
//! schema drop is provably complete — no table can be missed — at the
//! cost of re-running migrations (~1-2 s in dev). Worth it: there's no
//! list to maintain.
//!
//! Refuses to run without explicit confirmation. Intended for the local
//! dev loop only — there is no "remote" mode.

use anyhow::{Context, Result};
use sqlx::Row;
use std::io::Write;
use tracing::{info, warn};

#[derive(clap::Args, Debug)]
pub struct FreshArgs {
    /// PostgreSQL database URL. Falls back to FC_DATABASE_URL env var
    /// or the default localhost connection.
    #[arg(
        long,
        env = "FC_DATABASE_URL",
        default_value = "postgresql://localhost:5432/flowcatalyst"
    )]
    pub database_url: String,

    /// Use the embedded PostgreSQL instance (the one fc-dev starts at
    /// :15432 in `~/.cache/flowcatalyst-dev/pgdata/`). Default true so
    /// `fc-dev fresh` matches `fc-dev` (start) without extra flags.
    #[cfg(feature = "embedded-db")]
    #[arg(long, env = "FC_EMBEDDED_DB", default_value = "true")]
    pub embedded_db: bool,

    /// Skip the interactive confirmation. ONLY pass this when scripted —
    /// fresh is destructive and irreversible.
    #[arg(long)]
    pub yes: bool,

    /// Email of the bootstrap admin to recreate after the schema rebuild.
    /// Defaults to the documented dev value so `fc-dev fresh` produces a
    /// known-good login.
    #[arg(long, default_value = "admin@flowcatalyst.local")]
    pub admin_email: String,

    /// Password of the bootstrap admin. Defaults to the documented dev
    /// value. Falls back to a relaxed complexity policy if the value
    /// doesn't meet the strict one (matching `bootstrap_admin_user`'s
    /// own fallback so the password you set is the password you can use).
    #[arg(long, default_value = "DevPassword123!")]
    pub admin_password: String,
}

pub async fn run(args: FreshArgs) -> Result<()> {
    // Embedded PG, if requested. Same data dir as the start path so the
    // reset hits the right database.
    #[cfg(feature = "embedded-db")]
    let (db_url, mut _embedded) = if args.embedded_db {
        let emb = crate::embedded_pg::start(false).await?;
        let url = emb.url.clone();
        (url, Some(emb))
    } else {
        (args.database_url.clone(), None)
    };
    #[cfg(not(feature = "embedded-db"))]
    let db_url = args.database_url.clone();

    let pool = fc_platform::shared::database::create_pool(&db_url)
        .await
        .context("connect to database")?;

    // Show the user exactly what's about to vanish before asking for
    // confirmation. We don't filter by name prefix any more — the schema
    // drop will take every table in `public`, so list every table.
    let tables_before = list_public_tables(&pool).await?;

    println!(
        "About to DROP and recreate the `public` schema in {}.",
        db_url
    );
    if tables_before.is_empty() {
        println!("(no tables present — the schema will be initialised from scratch)");
    } else {
        println!(
            "This wipes {} table(s) including all data:",
            tables_before.len()
        );
        for t in &tables_before {
            println!("  - {}", t);
        }
    }
    println!(
        "\nAfter the drop, every migration re-runs and the dev seeders re-create\n\
         built-in roles, the `platform` application, default processes, and the\n\
         bootstrap admin ({}).\n",
        args.admin_email
    );

    if !args.yes {
        print!("Type \"fresh\" to confirm: ");
        std::io::stdout().flush().ok();
        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .context("read confirmation")?;
        if input.trim() != "fresh" {
            println!("Aborted — confirmation did not match.");
            #[cfg(feature = "embedded-db")]
            if let Some(mut e) = _embedded {
                crate::embedded_pg::stop(&mut e).await;
            }
            return Ok(());
        }
    }

    info!(
        table_count = tables_before.len(),
        "Dropping and recreating public schema"
    );

    // Two statements — sqlx's `query()` is a prepared statement and
    // rejects multi-command SQL, so run them individually.
    if let Err(e) = sqlx::query("DROP SCHEMA public CASCADE")
        .execute(&pool)
        .await
    {
        warn!(error = %e, "DROP SCHEMA failed");
        return Err(anyhow::anyhow!("DROP SCHEMA failed: {}", e));
    }
    if let Err(e) = sqlx::query("CREATE SCHEMA public").execute(&pool).await {
        warn!(error = %e, "CREATE SCHEMA failed");
        return Err(anyhow::anyhow!("CREATE SCHEMA failed: {}", e));
    }

    info!("Re-running migrations");
    fc_platform::shared::database::run_migrations(
        &pool,
        fc_platform::shared::database::MigrationProfile::Embedded,
    )
    .await
    .context("re-run migrations after schema drop")?;

    info!("Re-seeding built-in roles, platform application, default processes, admin user");

    fc_platform::shared::database::seed_builtin_roles(&pool)
        .await
        .context("seed built-in roles")?;
    fc_platform::shared::database::seed_platform_application(&pool)
        .await
        .context("seed platform application")?;
    fc_platform::shared::database::seed_platform_event_types(&pool)
        .await
        .context("seed platform event types")?;
    fc_platform::shared::default_processes::seed_default_processes(&pool)
        .await
        .context("seed default processes")?;

    // `bootstrap_admin_user` reads its config from env vars. Setting
    // them here (only for this process) keeps the seeder's existing
    // single source of truth without splitting the API in two.
    std::env::set_var("FLOWCATALYST_BOOTSTRAP_ADMIN_EMAIL", &args.admin_email);
    std::env::set_var(
        "FLOWCATALYST_BOOTSTRAP_ADMIN_PASSWORD",
        &args.admin_password,
    );
    fc_platform::shared::bootstrap_admin::bootstrap_admin_user(&pool)
        .await
        .context("bootstrap admin user")?;

    println!("\n✓ Schema dropped, migrations re-run, dev defaults seeded.");
    println!(
        "  Admin login:  {} / {}",
        args.admin_email, args.admin_password
    );
    println!("  Start fc-dev and sign in at http://localhost:8080.");

    #[cfg(feature = "embedded-db")]
    if let Some(mut e) = _embedded {
        crate::embedded_pg::stop(&mut e).await;
    }
    Ok(())
}

/// Enumerate every table in the `public` schema. Used only to render the
/// confirmation banner — the drop itself doesn't filter, it takes
/// everything.
async fn list_public_tables(pool: &sqlx::PgPool) -> Result<Vec<String>> {
    let rows = sqlx::query(
        "SELECT tablename FROM pg_tables \
         WHERE schemaname = 'public' \
         ORDER BY tablename",
    )
    .fetch_all(pool)
    .await
    .context("query pg_tables")?;
    Ok(rows
        .into_iter()
        .filter_map(|r| r.try_get::<String, _>("tablename").ok())
        .collect())
}
