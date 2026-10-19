//! Database connection and migrations (fc-platform-core), and the
//! platform's startup seeding.
//!
//! The pool, the secret refresh and the migration runner are
//! fc-platform-core's, re-exported here at their old paths.
//! [`run_migrations`] runs them with the platform's code migrations; the
//! seed functions write the built-in application, event types and roles.

pub use fc_platform_core::shared::database::*;

use crate::event_type::repository::EventTypeRepository;
use crate::scheduled_job::cron_migration;
use crate::seed::platform_event_types;
use crate::shared::error;
use sqlx::PgPool;
use tracing::info;

/// Run every migration: the SQL ones, then the migrations written in Rust
/// (the scheduled-job cron rewrite, 036). See
/// [`run_migrations_with`](fc_platform_core::shared::database::run_migrations_with).
pub async fn run_migrations(pool: &PgPool, profile: MigrationProfile) -> Result<(), sqlx::Error> {
    run_migrations_with(pool, profile, &[&CronMigration]).await
}

/// 036: the one-off rewrite of crons the previous Rust poller read in the
/// `cron` crate's dialect into Java/Go's. It needs the old parser, so it
/// is Rust; it tracks itself in `_schema_migrations` (never on a
/// database another platform has migrated: see the module docs). It is
/// not in the pre-tracker backfill on purpose: a pre-tracker Rust
/// database still needs it, and its own guard covers Go's.
struct CronMigration;

#[async_trait::async_trait]
impl CodeMigration for CronMigration {
    async fn run(&self, pool: &PgPool) -> Result<(), sqlx::Error> {
        cron_migration::run(pool).await.map(|_| ())
    }
}

// ── Built-in role seeding ────────────────────────────────────────────────────

/// Ensure the platform's built-in roles (defined in `role::entity::roles::all()`)
/// exist in `iam_roles`. Called on every startup.
///
/// **Upsert-only, no reconciliation:** inserts missing rows, leaves existing
/// rows alone. If an admin renames or deletes a built-in role at runtime, this
/// won't resurrect it — that's intentional. Built-in role definitions in code
/// are the platform's **initial state**, not an authoritative mirror.
///
/// Permissions for newly-inserted roles are also seeded from code.
/// Ensure the special `platform` application row exists. The Developer
/// portal treats the platform itself as one of the applications: the
/// dynamic utoipa-generated OpenAPI document is stored against this row by
/// the "Sync All" dashboard action. Idempotent — leaves any existing row
/// alone (including the more-descriptive name the dev seeder may have set).
pub async fn seed_platform_application(pool: &PgPool) -> error::Result<()> {
    use crate::application::entity::Application;
    use crate::application::repository::ApplicationRepository;

    let repo = ApplicationRepository::new(pool);
    if repo.find_by_code("platform").await?.is_some() {
        return Ok(());
    }

    let app = Application::new("platform", "FlowCatalyst Platform").with_description(
        "Core platform — its own OpenAPI document is published here as one of the applications",
    );
    repo.insert(&app).await?;
    info!("Seeded built-in platform application");
    Ok(())
}

/// Seed the platform's event-type catalogue, as Go does on every start
/// (`seed/event_types.go` `seedPlatformEventTypes`). See
/// [`EventTypeRepository::seed_catalogue`](crate::event_type::repository::EventTypeRepository::seed_catalogue).
///
/// Bootstrap-only, like [`seed_builtin_roles`]: it runs before HTTP serving
/// begins, has no executing principal, and writes no events (see CLAUDE.md
/// "Built-in role seeding").
pub async fn seed_platform_event_types(pool: &PgPool) -> error::Result<()> {
    let repo = EventTypeRepository::new(pool);
    let defs = platform_event_types::definitions();
    let inserted = repo.seed_catalogue(&defs).await?;
    if inserted > 0 {
        info!(inserted, total = defs.len(), "Seeded platform event types");
    }
    Ok(())
}

/// Install, and on every boot re-assert, the built-in PUBLIC OAuth client
/// behind `fcdev login` (Go F25 `seed.SeedFcdevCLIClient`).
///
/// Security-relevant columns and junctions are overwritten on every start so a
/// console edit can never widen the client: public (no secret), PKCE required,
/// `api_access` (the user's own roles become the token's authority),
/// `authorization_code` + `refresh_token` only, the port-less loopback
/// redirects, no application scoping, no allowed origins, not a portal client,
/// no service-account link. Only `active` is left alone, so an operator can
/// switch fcdev logins off by deactivating the client and a restart will not
/// turn them back on.
///
/// Bootstrap-only, like [`seed_builtin_roles`]: no executing principal, so it
/// writes directly and emits no event (see CLAUDE.md "Built-in role seeding").
pub async fn seed_fcdev_cli_client(pool: &PgPool) -> error::Result<()> {
    use crate::auth::fcdev;
    use fc_platform_core::shared::id::OAuthClientId;

    let mut tx = pool.begin().await?;
    let (id,): (String,) = sqlx::query_as(
        r#"INSERT INTO oauth_clients
               (id, client_id, client_name, client_type, client_secret_ref, default_scopes,
                pkce_required, service_account_principal_id, active, created_at, updated_at,
                portal_client_id, api_access, portal_app_id,
                previous_secret_ref, previous_secret_expires_at, previous_secret_last_used_at)
           VALUES ($1, $2, $3, 'PUBLIC', NULL, NULL, TRUE, NULL, TRUE, NOW(), NOW(),
                   NULL, TRUE, NULL, NULL, NULL, NULL)
           ON CONFLICT (client_id) DO UPDATE SET
               client_name = EXCLUDED.client_name,
               client_type = 'PUBLIC',
               client_secret_ref = NULL,
               previous_secret_ref = NULL,
               previous_secret_expires_at = NULL,
               previous_secret_last_used_at = NULL,
               default_scopes = NULL,
               pkce_required = TRUE,
               service_account_principal_id = NULL,
               portal_client_id = NULL,
               portal_app_id = NULL,
               api_access = TRUE,
               updated_at = EXCLUDED.updated_at
           RETURNING id"#,
    )
    .bind(OAuthClientId::generate().as_str())
    .bind(fcdev::FCDEV_CLI_CLIENT_ID)
    .bind(fcdev::FCDEV_CLI_CLIENT_NAME)
    .fetch_one(&mut *tx)
    .await?;
    for table in [
        "oauth_client_redirect_uris",
        "oauth_client_grant_types",
        "oauth_client_application_ids",
        "oauth_client_allowed_origins",
        "oauth_client_post_logout_redirect_uris",
    ] {
        sqlx::query(&format!("DELETE FROM {table} WHERE oauth_client_id = $1"))
            .bind(&id)
            .execute(&mut *tx)
            .await?;
    }
    for uri in fcdev::FCDEV_CLI_REDIRECT_URIS {
        sqlx::query(
            "INSERT INTO oauth_client_redirect_uris (oauth_client_id, redirect_uri) VALUES ($1, $2)",
        )
        .bind(&id)
        .bind(uri)
        .execute(&mut *tx)
        .await?;
    }
    for grant in fcdev::FCDEV_CLI_GRANT_TYPES {
        sqlx::query(
            "INSERT INTO oauth_client_grant_types (oauth_client_id, grant_type) VALUES ($1, $2)",
        )
        .bind(&id)
        .bind(grant)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    tracing::debug!("fcdev-cli OAuth client asserted");
    Ok(())
}

pub async fn seed_builtin_roles(pool: &PgPool) -> error::Result<()> {
    use crate::role::entity::roles;
    use crate::role::repository::RoleRepository;

    let repo = RoleRepository::new(pool);
    let mut inserted = 0;

    for role in roles::all() {
        if repo.find_by_name(&role.name).await?.is_some() {
            continue;
        }
        repo.insert(&role).await?;
        info!(role = %role.name, "Seeded built-in role");
        inserted += 1;
    }

    if inserted > 0 {
        info!(count = inserted, "Built-in role seeding complete");
    }
    Ok(())
}
