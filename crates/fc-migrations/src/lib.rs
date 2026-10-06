//! The SQL migration runner: the ordered list of `migrations/*.sql`, the
//! `_schema_migrations` tracker (checksums, drift warning, pre-tracker
//! backfill) and the [`CodeMigration`] hook.
//!
//! This is its own crate, with no checked (`query!`) queries, so that
//! `scripts/sqlx-prepare.sh` can build and run `fc-migrate` to migrate a
//! throwaway database *before* the crates that hold checked queries can be
//! compiled (they need a migrated database or an up-to-date `.sqlx/`).
//! `fc_platform_core::shared::database` re-exports everything here.

use sqlx::PgPool;
use std::mem;
use std::time::Instant;
use tracing::{info, warn};

// ── Migrations ───────────────────────────────────────────────────────────────

/// Migration profile. Selects which optional migrations apply.
///
/// `Embedded` is for local dev (`fc-dev`) using `postgresql_embedded`. It skips
/// production-only migrations like declarative partitioning, which add
/// operational machinery (partition manager, retention sweeps) that aren't
/// useful when the data dir is throwaway.
///
/// `Production` is for `fc-server` and any RDS-backed deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationProfile {
    Embedded,
    Production,
}

/// The migrations applied to every profile, in order: `(id, SQL)`.
///
/// A migration that is no longer applied is not deleted from here silently:
/// it moves to [`RETIRED_MIGRATIONS`], which says what replaced it.
fn core_migrations() -> &'static [(&'static str, &'static str)] {
    &[
        (
            "001_tenant_tables",
            include_str!("../../../migrations/001_tenant_tables.sql"),
        ),
        (
            "002_iam_tables",
            include_str!("../../../migrations/002_iam_tables.sql"),
        ),
        (
            "003_application_tables",
            include_str!("../../../migrations/003_application_tables.sql"),
        ),
        (
            "004_messaging_tables",
            include_str!("../../../migrations/004_messaging_tables.sql"),
        ),
        (
            "005_outbox_tables",
            include_str!("../../../migrations/005_outbox_tables.sql"),
        ),
        (
            "006_audit_tables",
            include_str!("../../../migrations/006_audit_tables.sql"),
        ),
        (
            "007_oauth_tables",
            include_str!("../../../migrations/007_oauth_tables.sql"),
        ),
        (
            "008_auth_tracking_tables",
            include_str!("../../../migrations/008_auth_tracking_tables.sql"),
        ),
        (
            "009_p0_alignment",
            include_str!("../../../migrations/009_p0_alignment.sql"),
        ),
        (
            "010_auth_state_tables",
            include_str!("../../../migrations/010_auth_state_tables.sql"),
        ),
        (
            "011_dispatch_job_tables",
            include_str!("../../../migrations/011_dispatch_job_tables.sql"),
        ),
        (
            "012_projection_columns",
            include_str!("../../../migrations/012_projection_columns.sql"),
        ),
        (
            "013_drop_connection_endpoint",
            include_str!("../../../migrations/013_drop_connection_endpoint.sql"),
        ),
        (
            "014_widen_attempt_type",
            include_str!("../../../migrations/014_widen_attempt_type.sql"),
        ),
        (
            "015_dispatch_jobs_write_indexes",
            include_str!("../../../migrations/015_dispatch_jobs_write_indexes.sql"),
        ),
        (
            "016_clean_orphaned_role_assignments",
            include_str!("../../../migrations/016_clean_orphaned_role_assignments.sql"),
        ),
        (
            "017_dispatch_pool_rate_limit_nullable",
            include_str!("../../../migrations/017_dispatch_pool_rate_limit_nullable.sql"),
        ),
        // 018 reshapes the messaging tables into the partitioning-ready
        // schema (composite PKs, fanned_out_at, read-table created_at).
        (
            "018_partition_prep",
            include_str!("../../../migrations/018_partition_prep.sql"),
        ),
        // 019/022 partition the high-volume tables. They used to be
        // production-only, but fc-dev now mirrors prod's partitioned shape so
        // partition-related schema bugs (UNIQUE missing the partition key,
        // queries without it in WHERE) surface in dev rather than getting
        // discovered in prod. Forward-rolling and retention are managed by
        // pg_partman_bgw in production (registered in 023) and by
        // `PartitionManagerService` in fc-dev.
        (
            "019_partition_messaging_tables",
            include_str!("../../../migrations/019_partition_messaging_tables.sql"),
        ),
        (
            "020_webauthn_credentials",
            include_str!("../../../migrations/020_webauthn_credentials.sql"),
        ),
        (
            "021_scheduled_jobs",
            include_str!("../../../migrations/021_scheduled_jobs.sql"),
        ),
        (
            "022_partition_scheduled_job_history",
            include_str!("../../../migrations/022_partition_scheduled_job_history.sql"),
        ),
        // Bridges DBs that ran 021 before `target_url` was added to it.
        (
            "024_scheduled_jobs_add_target_url",
            include_str!("../../../migrations/024_scheduled_jobs_add_target_url.sql"),
        ),
        (
            "025_application_openapi_specs",
            include_str!("../../../migrations/025_application_openapi_specs.sql"),
        ),
        (
            "026_processes",
            include_str!("../../../migrations/026_processes.sql"),
        ),
        // Wires the FK from oauth_clients.service_account_principal_id back
        // to iam_principals (CASCADE), and deletes orphaned clients left
        // behind by SA deletes that pre-dated this constraint. Idempotent:
        // drops the constraint before re-adding.
        (
            "027_oauth_clients_service_account_fk",
            include_str!("../../../migrations/027_oauth_clients_service_account_fk.sql"),
        ),
        // Wires the FK from app_applications.service_account_id back to
        // iam_principals (SET NULL so the application survives SA delete),
        // and clears any dangling references so a replacement SA can be
        // provisioned.
        (
            "028_application_service_account_fk",
            include_str!("../../../migrations/028_application_service_account_fk.sql"),
        ),
        (
            "029_oauth_client_post_logout_redirect_uris",
            include_str!("../../../migrations/029_oauth_client_post_logout_redirect_uris.sql"),
        ),
        (
            "030_rate_limit_events",
            include_str!("../../../migrations/030_rate_limit_events.sql"),
        ),
        // Go's 034: the stored all-applications flag on principals.
        (
            "031_principal_all_applications",
            include_str!("../../../migrations/031_principal_all_applications.sql"),
        ),
        // The iam_service_accounts columns of Go's 035: the requested scope
        // and the client links, stored on the service account.
        (
            "032_service_account_scope_and_client_ids",
            include_str!("../../../migrations/032_service_account_scope_and_client_ids.sql"),
        ),
        // Go's 046 + 047: the OAuth client secret-rotation overlap.
        (
            "033_oauth_client_secret_grace",
            include_str!("../../../migrations/033_oauth_client_secret_grace.sql"),
        ),
        // 034_functions (Java's V13 + V15 - V16, the fn_* registry) is
        // retired: see RETIRED_MIGRATIONS and 062.
        // Java's msg_scheduled_jobs.application_id: a function's schedules
        // are signed with its application's credentials.
        (
            "035_scheduled_jobs_application_id",
            include_str!("../../../migrations/035_scheduled_jobs_application_id.sql"),
        ),
        // 036 is a data migration in Rust, not SQL, run after these:
        // `036_scheduled_job_cron_dialect` (see below).
        // 037_function_component_runtime is retired: see RETIRED_MIGRATIONS.
        // Java's V18: aud_logs.entity_id widened to 100, so a sync rollup's
        // audit row (keyed by the application code) fits.
        (
            "038_aud_logs_entity_id_width",
            include_str!("../../../migrations/038_aud_logs_entity_id_width.sql"),
        ),
        // Go's 054 + 057 (the parts the dispatch pipeline needs): a job's
        // own queue priority, and what an attempt sent.
        (
            "039_dispatch_job_queue_and_attempt_request",
            include_str!("../../../migrations/039_dispatch_job_queue_and_attempt_request.sql"),
        ),
        // Go's 031: two-factor authentication (factors, recovery codes,
        // email PINs, trusted devices, the per-domain policy).
        (
            "041_mfa_tables",
            include_str!("../../../migrations/041_mfa_tables.sql"),
        ),
        // The reset-token columns of Go's 031, 032, 033, 041 and 051.
        (
            "042_password_reset_token_purpose",
            include_str!("../../../migrations/042_password_reset_token_purpose.sql"),
        ),
        // Go's 042: the OAuth client flag that makes an interactive login's
        // access token authority-bearing (owner decisions #3/#20).
        (
            "043_oauth_client_api_access",
            include_str!("../../../migrations/043_oauth_client_api_access.sql"),
        ),
        // Go's 032: the lost-device reset approval queue.
        (
            "044_reset_approval_requests",
            include_str!("../../../migrations/044_reset_approval_requests.sql"),
        ),
        // Go's 041 + 043: the portal identity plane (portal_identities,
        // portal_login_flows, the portal flags on OAuth clients and OIDC
        // login states, the reset-token redirect).
        (
            "046_portal_identities",
            include_str!("../../../migrations/046_portal_identities.sql"),
        ),
        // Go's 053: portal apps and per-app grants.
        (
            "047_portal_apps",
            include_str!("../../../migrations/047_portal_apps.sql"),
        ),
        // Go's 056: application-scoped connections (application_code,
        // source) and the (application_code, client_id, code) uniqueness.
        (
            "050_connection_application_scope",
            include_str!("../../../migrations/050_connection_application_scope.sql"),
        ),
        // Go's 044: application-synced documentation.
        (
            "051_app_docs",
            include_str!("../../../migrations/051_app_docs.sql"),
        ),
        // Go's 039: the self-service developer API credential.
        (
            "052_developer_api_credentials",
            include_str!("../../../migrations/052_developer_api_credentials.sql"),
        ),
        // Go's 035, the subscription half: msg_subscriptions.created_by.
        (
            "053_subscription_created_by",
            include_str!("../../../migrations/053_subscription_created_by.sql"),
        ),
        // Go's 035 (part): the event type's creator.
        (
            "054_event_type_created_by",
            include_str!("../../../migrations/054_event_type_created_by.sql"),
        ),
        // Go's 040: role sync configured on the identity provider (the
        // flag and the allowed-roles junction).
        (
            "055_identity_provider_role_sync",
            include_str!("../../../migrations/055_identity_provider_role_sync.sql"),
        ),
        // 056_function_js_runtime is retired: see RETIRED_MIGRATIONS.
        // Go's 057 (the rest of it): a dispatch job's descriptor, and the
        // job's descriptor and metadata on the read projection.
        (
            "057_dispatch_job_descriptor_and_read_metadata",
            include_str!("../../../migrations/057_dispatch_job_descriptor_and_read_metadata.sql"),
        ),
        // An application's per-client base-URL override and configuration
        // document, which Go documents but never stores.
        (
            "058_app_client_config_overrides",
            include_str!("../../../migrations/058_app_client_config_overrides.sql"),
        ),
        // Who assigned a role, which Go documents on a service account's
        // role assignments but never stores.
        (
            "059_principal_role_assigned_by",
            include_str!("../../../migrations/059_principal_role_assigned_by.sql"),
        ),
        // The webhook-credential members Go accepts on a service account
        // but drops (username, password, header names).
        (
            "060_service_account_webhook_credential_members",
            include_str!("../../../migrations/060_service_account_webhook_credential_members.sql"),
        ),
        // A dispatch job's own priority claim on the read projection, for
        // the list row's `priority` (Go documents it, never fills it).
        (
            "061_dispatch_job_read_queue",
            include_str!("../../../migrations/061_dispatch_job_read_queue.sql"),
        ),
        // Owner decision #48: the function registry as `fnr_*`, the end
        // state of the retired 034 + 037 + 056 renamed, and
        // msg_subscriptions.source admitting FUNCTION.
        (
            "062_function_registry_fnr",
            include_str!("../../../migrations/062_function_registry_fnr.sql"),
        ),
        // msg_dispatch_jobs indexes rebuilt around the statements that use
        // them: the claim's total order (no sort), the hold-back checks,
        // the stale / reaper sweeps, and the projector's dirty predicate.
        (
            "063_dispatch_job_scheduler_indexes",
            include_str!("../../../migrations/063_dispatch_job_scheduler_indexes.sql"),
        ),
        // msg_dispatch_queue: one row per PENDING dispatch job, kept exact by
        // the dispatch-job lifecycle's writes. Nothing reads it yet.
        (
            "064_dispatch_queue",
            include_str!("../../../migrations/064_dispatch_queue.sql"),
        ),
        // The scheduler claims from msg_dispatch_queue: the partial indexes
        // the dispatch path read msg_dispatch_jobs through go, one ordinary
        // status index replaces them, the queue table gets its storage
        // options, and any drift an older binary left is repaired.
        (
            "065_dispatch_queue_reads",
            include_str!("../../../migrations/065_dispatch_queue_reads.sql"),
        ),
        // The queue table is retired: the scheduler claims from
        // msg_dispatch_jobs again and nothing reads or writes it.
        (
            "066_drop_dispatch_queue",
            include_str!("../../../migrations/066_drop_dispatch_queue.sql"),
        ),
    ]
}

/// Migrations the runner no longer applies: `(id, what replaced it)`.
///
/// A retired migration's file stays in `migrations/`, unchanged (shipped
/// migrations are immutable), but it is not in [`core_migrations`], so:
///
/// - **it never runs**: on a fresh database, or one another platform
///   migrated, none of its SQL executes;
/// - **a recorded row is tolerated**: a database that applied it keeps its
///   `_schema_migrations` row and checksum. Drift detection only compares
///   the migrations in the list, so the row is never checked, never
///   re-run and never deleted; [`run_migrations`] logs that it is ignored;
/// - **the pre-tracker backfill skips it**: no row is recorded for it, so
///   nothing claims it ran where it did not.
///
/// Whatever it created stays in place; Rust just stops using it. Its
/// replacement is an ordinary migration that brings every database to the
/// state Rust now needs.
///
/// Owner decision #48 (2026-09-28): Rust's function registry moved from
/// `fn_*`, which Java also uses and Go's own `fn_*` tables collide with, to
/// `fnr_*`, until the owner picks one implementation. The three migrations
/// that created and altered Rust's `fn_*` tables are retired, so Rust never
/// creates or alters a `fn_*` table again (on a given database those are
/// Java's or Go's), and 062 creates `fnr_*` in their end state.
pub(crate) const RETIRED_MIGRATIONS: &[(&str, &str)] = &[
    ("034_functions", "062_function_registry_fnr"),
    (
        "037_function_component_runtime",
        "062_function_registry_fnr",
    ),
    ("056_function_js_runtime", "062_function_registry_fnr"),
];

/// A migration written in Rust: it runs on every start and tracks itself
/// (in `_schema_migrations`), as the SQL ones are tracked.
#[async_trait::async_trait]
pub trait CodeMigration: Sync {
    async fn run(&self, pool: &PgPool) -> Result<(), sqlx::Error>;
}

/// Run all SQL migrations from the migrations/ directory.
///
/// Each migration is applied at most once. Tracking lives in
/// `_schema_migrations`; SQL execution and the tracker INSERT happen in the
/// same transaction, so a partial migration never gets marked applied.
///
/// First-run on a pre-tracker DB: if the tracker is empty but a legacy
/// table (`tnt_clients` from 001) exists, every defined migration is
/// marked applied so the no-tracker era's idempotent migrations don't
/// get re-run on top of schema mutations they predate (e.g. running
/// migration 4's `CREATE UNIQUE INDEX … (deduplication_id)` on top of
/// a partitioned `msg_events`).
///
/// `code_migrations` run after the SQL ones, in order: migrations written in
/// Rust that live with the aggregate they rewrite (the platform passes the
/// scheduled-job cron migration, 036; see
/// `fc_platform::shared::database::run_migrations`).
pub async fn run_migrations_with(
    pool: &PgPool,
    profile: MigrationProfile,
    code_migrations: &[&dyn CodeMigration],
) -> Result<(), sqlx::Error> {
    info!(?profile, "Running database migrations...");

    let core_migrations = core_migrations();

    // No production-only migrations at the moment. Partitioning runs the
    // same way in every profile — bootstrapped by 019/022 (both core) and
    // maintained by `fc_stream::PartitionManagerService` everywhere.
    let production_migrations: &[(&str, &str)] = &[];

    // Bootstrap the tracker. CREATE IF NOT EXISTS is safe to re-run.
    // The `checksum` column lets us detect drift — i.e. a migration whose
    // SQL has been edited after it was applied. Editing shipped migrations
    // is a sharp edge: the tracker treats them as immutable, so the new SQL
    // never runs, and we'd silently miss schema changes. Instead we store
    // a sha256 of each migration's contents on apply and warn loudly if a
    // later run sees a different hash for the same id.
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS _schema_migrations (
            migration_id VARCHAR(100) PRIMARY KEY,
            applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            duration_ms INTEGER,
            checksum TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;
    // For DBs whose tracker was created by an earlier build that didn't
    // have the checksum column yet.
    sqlx::query("ALTER TABLE _schema_migrations ADD COLUMN IF NOT EXISTS checksum TEXT")
        .execute(pool)
        .await?;

    // Per-migration probes for the recently-added migrations: a SQL
    // expression returning true iff the migration's effects are visible.
    // Older migrations (001–019) are assumed applied if the legacy `tnt_clients`
    // table exists (any pre-tracker prod DB was already running them every
    // deploy). Recent migrations need their own probe because they may have
    // been defined but not yet successfully applied — the deploy that broke
    // on migration 4 is the canonical example.
    let probes: &[(&str, &str)] = &[
        (
            "020_webauthn_credentials",
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_name = 'webauthn_credentials')",
        ),
        (
            "021_scheduled_jobs",
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_name = 'msg_scheduled_jobs')",
        ),
        (
            "022_partition_scheduled_job_history",
            "SELECT EXISTS (SELECT 1 FROM pg_partitioned_table pt \
             JOIN pg_class c ON c.oid = pt.partrelid \
             WHERE c.relname = 'msg_scheduled_job_instances')",
        ),
        (
            "025_application_openapi_specs",
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_name = 'app_application_openapi_specs')",
        ),
        // FK constraints: probe `information_schema.table_constraints` for
        // the constraint name added in the migration's `ADD CONSTRAINT`.
        (
            "027_oauth_clients_service_account_fk",
            "SELECT EXISTS (SELECT 1 FROM information_schema.table_constraints \
             WHERE table_schema = 'public' \
               AND table_name = 'oauth_clients' \
               AND constraint_name = 'oauth_clients_service_account_fk')",
        ),
        (
            "028_application_service_account_fk",
            "SELECT EXISTS (SELECT 1 FROM information_schema.table_constraints \
             WHERE table_schema = 'public' \
               AND table_name = 'app_applications' \
               AND constraint_name = 'app_applications_service_account_fk')",
        ),
        (
            "029_oauth_client_post_logout_redirect_uris",
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_name = 'oauth_client_post_logout_redirect_uris')",
        ),
        (
            "030_rate_limit_events",
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_name = 'iam_rate_limit_events')",
        ),
        // Column additions: probe `information_schema.columns`. A database
        // Go has migrated already has it.
        (
            "031_principal_all_applications",
            "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' \
               AND table_name = 'iam_principals' \
               AND column_name = 'all_applications')",
        ),
        (
            "032_service_account_scope_and_client_ids",
            "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' \
               AND table_name = 'iam_service_accounts' \
               AND column_name = 'client_ids')",
        ),
        // Go's 047 is the later of the two, so its column means both ran.
        (
            "033_oauth_client_secret_grace",
            "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' \
               AND table_name = 'oauth_clients' \
               AND column_name = 'previous_secret_last_used_at')",
        ),
        // A database Java migrated has the column from its baseline.
        (
            "035_scheduled_jobs_application_id",
            "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' \
               AND table_name = 'msg_scheduled_jobs' \
               AND column_name = 'application_id')",
        ),
        // A database Java migrated to V18 already has the width.
        (
            "038_aud_logs_entity_id_width",
            "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'aud_logs' \
               AND column_name = 'entity_id' \
               AND character_maximum_length >= 100)",
        ),
        // A database Go migrated to 057 already has both columns.
        (
            "039_dispatch_job_queue_and_attempt_request",
            "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'msg_dispatch_jobs' \
               AND column_name = 'queue') \
             AND EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'msg_dispatch_job_attempts' \
               AND column_name = 'request_info')",
        ),
        // A database Go migrated has the last of the tables and the policy
        // junction.
        (
            "041_mfa_tables",
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_name = 'iam_mfa_trusted_devices') \
             AND EXISTS (SELECT 1 FROM information_schema.tables \
             WHERE table_schema = 'public' \
               AND table_name = 'tnt_email_domain_mapping_2fa_methods')",
        ),
        // Go's 051 CHECK is the last of the reset-token changes.
        (
            "042_password_reset_token_purpose",
            "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'iam_password_reset_tokens' \
               AND column_name = 'redirect_uri') \
             AND EXISTS (SELECT 1 FROM pg_constraint \
             WHERE conname = 'chk_iam_password_reset_tokens_purpose')",
        ),
        // A database Go migrated (its 042) already has the column.
        (
            "043_oauth_client_api_access",
            "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'oauth_clients' \
               AND column_name = 'api_access')",
        ),
        (
            "044_reset_approval_requests",
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_name = 'iam_reset_approval_requests')",
        ),
        // A database Go migrated past its 041 has both the flow table and
        // the reset-token redirect column.
        (
            "046_portal_identities",
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_name = 'portal_login_flows') \
             AND EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'iam_password_reset_tokens' \
               AND column_name = 'redirect_uri')",
        ),
        // Go's 053 adds the invite columns last-but-one; with the grants
        // table they mean it ran.
        (
            "047_portal_apps",
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_name = 'portal_identity_apps') \
             AND EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'portal_identities' \
               AND column_name = 'invite_expires_at')",
        ),
        // A database Go migrated to 056 has the column and the new index.
        (
            "050_connection_application_scope",
            "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'msg_connections' \
               AND column_name = 'source') \
             AND EXISTS (SELECT 1 FROM pg_indexes \
             WHERE schemaname = 'public' \
               AND indexname = 'uq_msg_subscriptions_app_client_code')",
        ),
        // A database Go migrated to 044 has the table.
        (
            "051_app_docs",
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_name = 'app_docs')",
        ),
        (
            "052_developer_api_credentials",
            "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'iam_principals' \
               AND column_name = 'dev_client_secret_updated_at')",
        ),
        (
            "053_subscription_created_by",
            "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'msg_subscriptions' \
               AND column_name = 'created_by')",
        ),
        (
            "054_event_type_created_by",
            "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'msg_event_types' \
               AND column_name = 'created_by')",
        ),
        // A database Go migrated to 040 has the flag and the junction.
        (
            "055_identity_provider_role_sync",
            "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'oauth_identity_providers' \
               AND column_name = 'sync_roles_from_idp') \
             AND EXISTS (SELECT 1 FROM information_schema.tables \
             WHERE table_schema = 'public' \
               AND table_name = 'oauth_identity_provider_allowed_roles')",
        ),
        // A database Go migrated to 057 has all three columns.
        (
            "057_dispatch_job_descriptor_and_read_metadata",
            "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'msg_dispatch_jobs' \
               AND column_name = 'descriptor') \
             AND EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'msg_dispatch_jobs_read' \
               AND column_name = 'descriptor') \
             AND EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'msg_dispatch_jobs_read' \
               AND column_name = 'metadata')",
        ),
        (
            "058_app_client_config_overrides",
            "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'app_client_configs' \
               AND column_name = 'config_json')",
        ),
        (
            "059_principal_role_assigned_by",
            "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'iam_principal_roles' \
               AND column_name = 'assigned_by')",
        ),
        (
            "060_service_account_webhook_credential_members",
            "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'iam_service_accounts' \
               AND column_name = 'wh_signature_header')",
        ),
        (
            "061_dispatch_job_read_queue",
            "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'msg_dispatch_jobs_read' \
               AND column_name = 'queue')",
        ),
        // Only 062 creates fnr_*, in one transaction, so its last table
        // means it ran; the source CHECK is its other effect.
        (
            "062_function_registry_fnr",
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_name = 'fnr_secrets') \
             AND EXISTS (SELECT 1 FROM pg_constraint \
             WHERE conname = 'chk_msg_subscriptions_source' \
               AND pg_get_constraintdef(oid) LIKE '%''FUNCTION''%')",
        ),
        // 063 only swaps indexes; the last one it creates means it ran.
        (
            "063_dispatch_job_scheduler_indexes",
            "SELECT EXISTS (SELECT 1 FROM pg_indexes \
             WHERE schemaname = 'public' AND tablename = 'msg_dispatch_jobs' \
               AND indexname = 'idx_msg_dispatch_jobs_dirty')",
        ),
        // 064 creates the table and its index; the index means it ran. A
        // database another platform migrated all the way (Go 068 / Java V23)
        // has 065's index and no queue table: 064 ran there too, and must not
        // run again (it would recreate the table 066 retires).
        (
            "064_dispatch_queue",
            "SELECT EXISTS (SELECT 1 FROM pg_indexes \
             WHERE schemaname = 'public' AND tablename = 'msg_dispatch_queue' \
               AND indexname = 'idx_dispatch_queue_order') \
             OR (EXISTS (SELECT 1 FROM pg_indexes \
                 WHERE schemaname = 'public' AND tablename = 'msg_dispatch_jobs' \
                   AND indexname = 'idx_dispatch_jobs_status_group') \
                 AND NOT EXISTS (SELECT 1 FROM information_schema.tables \
                 WHERE table_schema = 'public' AND table_name = 'msg_dispatch_queue'))",
        ),
        // 065's new index means it ran (Go's 067 and Java's V22 create the
        // same index, so a database they migrated already has it).
        (
            "065_dispatch_queue_reads",
            "SELECT EXISTS (SELECT 1 FROM pg_indexes \
             WHERE schemaname = 'public' AND tablename = 'msg_dispatch_jobs' \
               AND indexname = 'idx_dispatch_jobs_status_group')",
        ),
        // 066 ran when 065's index exists and the queue table does not.
        (
            "066_drop_dispatch_queue",
            "SELECT EXISTS (SELECT 1 FROM pg_indexes \
             WHERE schemaname = 'public' AND tablename = 'msg_dispatch_jobs' \
               AND indexname = 'idx_dispatch_jobs_status_group') \
             AND NOT EXISTS (SELECT 1 FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_name = 'msg_dispatch_queue')",
        ),
    ];

    // Auto-backfill for pre-tracker DBs.
    let tracker_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM _schema_migrations")
        .fetch_one(pool)
        .await?;
    if tracker_count.0 == 0 {
        let legacy_present: (bool,) = sqlx::query_as(
            "SELECT EXISTS (
                SELECT 1 FROM information_schema.tables
                WHERE table_schema = 'public' AND table_name = 'tnt_clients'
            )",
        )
        .fetch_one(pool)
        .await?;
        if legacy_present.0 {
            warn!(
                "Pre-tracker DB detected (tnt_clients exists, _schema_migrations empty). \
                 Backfilling defined migrations whose effects are visible — recently-added \
                 migrations without visible effects will run on this deploy."
            );
            let mut tx = pool.begin().await?;
            let mut backfilled = 0;
            let mut skipped = Vec::new();
            for (id, sql) in core_migrations.iter().chain(production_migrations.iter()) {
                let probe_says_applied =
                    if let Some((_, probe_sql)) = probes.iter().find(|(p_id, _)| *p_id == *id) {
                        let r: (bool,) = sqlx::query_as(probe_sql).fetch_one(&mut *tx).await?;
                        r.0
                    } else {
                        true
                    };
                if probe_says_applied {
                    sqlx::query(
                        "INSERT INTO _schema_migrations (migration_id, checksum) VALUES ($1, $2) \
                         ON CONFLICT (migration_id) DO NOTHING",
                    )
                    .bind(*id)
                    .bind(sha256_hex(sql))
                    .execute(&mut *tx)
                    .await?;
                    backfilled += 1;
                } else {
                    skipped.push(*id);
                }
            }
            tx.commit().await?;
            info!(
                backfilled,
                skipped = ?skipped,
                "Backfill complete; skipped entries will run as fresh migrations"
            );
        } else {
            info!("Fresh DB — running all migrations.");
        }
    }

    // Retired migrations are never applied; a row one left behind stays as
    // it is (see RETIRED_MIGRATIONS).
    let retired_ids: Vec<&str> = RETIRED_MIGRATIONS.iter().map(|(id, _)| *id).collect();
    let recorded_retired: Vec<(String,)> = sqlx::query_as(
        "SELECT migration_id FROM _schema_migrations WHERE migration_id = ANY($1) \
         ORDER BY migration_id",
    )
    .bind(&retired_ids)
    .fetch_all(pool)
    .await?;
    for (id,) in recorded_retired {
        let replaced_by = RETIRED_MIGRATIONS
            .iter()
            .find(|(r, _)| *r == id)
            .map_or("", |(_, by)| *by);
        info!(
            migration = %id,
            replaced_by,
            "Retired migration recorded; ignored (not re-run, not drift-checked)"
        );
    }

    // Apply each migration if not already tracked.
    for (id, sql) in core_migrations.iter() {
        apply_tracked(pool, id, sql).await?;
    }
    if profile == MigrationProfile::Production {
        for (id, sql) in production_migrations.iter() {
            apply_tracked(pool, id, sql).await?;
        }
    }

    // The code migrations (036, the scheduled-job cron rewrite): each
    // tracks itself in `_schema_migrations`, and is not in the pre-tracker
    // backfill above on purpose (see its docs).
    for migration in code_migrations {
        migration.run(pool).await?;
    }

    info!("All database migrations completed");
    Ok(())
}

/// Apply one migration in its own transaction.
///
/// SQL execution and the `_schema_migrations` insert are atomic — if any
/// statement fails the whole thing rolls back, so the migration is not
/// marked applied and the next deploy retries it.
///
/// Drift detection: every migration is tracked with a sha256 of its SQL
/// content. Re-runs compare the current hash against the stored one:
/// - match → no-op (the normal case).
/// - stored is NULL → row predates the checksum column; silently backfill.
/// - mismatch → warn loudly (the migration's content was edited after it
///   was applied; the new SQL will NOT run, since migrations are immutable
///   once shipped). Operator should fix by writing a follow-up migration.
async fn apply_tracked(pool: &PgPool, id: &str, sql: &str) -> Result<(), sqlx::Error> {
    let current_checksum = sha256_hex(sql);

    let row: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT migration_id, checksum FROM _schema_migrations WHERE migration_id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    if let Some((_, tracked_checksum)) = row {
        match tracked_checksum {
            None => {
                // Pre-checksum row: backfill silently so future runs can
                // detect drift.
                sqlx::query("UPDATE _schema_migrations SET checksum = $1 WHERE migration_id = $2")
                    .bind(&current_checksum)
                    .bind(id)
                    .execute(pool)
                    .await?;
            }
            Some(stored) if stored == current_checksum => {
                // Match — already applied, content unchanged.
            }
            Some(stored) => {
                warn!(
                    migration = id,
                    stored_checksum = %stored,
                    current_checksum = %current_checksum,
                    "Migration content changed since it was applied. The new SQL has \
                     NOT been executed — migrations are immutable once shipped. If you \
                     intended a schema change, write a new migration. If the edit was \
                     benign (e.g. comment-only) you can silence this warning with: \
                     UPDATE _schema_migrations SET checksum = '<current>' WHERE migration_id = '<id>'."
                );
            }
        }
        return Ok(());
    }

    let mut tx = pool.begin().await?;
    let start = Instant::now();
    for statement in split_sql_statements(sql) {
        let cleaned: String = statement
            .lines()
            .filter(|line| !line.trim_start().starts_with("--"))
            .collect::<Vec<_>>()
            .join("\n");
        let trimmed = cleaned.trim();
        if trimmed.is_empty() {
            continue;
        }
        sqlx::query(trimmed).execute(&mut *tx).await?;
    }
    let duration_ms = start.elapsed().as_millis() as i32;
    sqlx::query(
        "INSERT INTO _schema_migrations (migration_id, duration_ms, checksum) \
         VALUES ($1, $2, $3)",
    )
    .bind(id)
    .bind(duration_ms)
    .bind(&current_checksum)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    info!(
        migration = id,
        duration_ms = duration_ms,
        "Migration applied"
    );
    Ok(())
}

/// SHA-256 of a migration's SQL body, hex-encoded. Used for drift
/// detection on re-runs of an already-applied migration.
pub fn sha256_hex(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

/// Split a SQL script into top-level statements on `;`, respecting:
/// - dollar-quoted bodies (`$$ ... $$` or `$tag$ ... $tag$`) used by `DO`/`CREATE FUNCTION`
/// - single-quoted strings (`'foo''bar'`)
/// - line comments (`-- ...`) and block comments (`/* ... */`)
fn split_sql_statements(sql: &str) -> Vec<String> {
    let bytes = sql.as_bytes();
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut i = 0;

    enum State {
        Normal,
        SingleQuote,
        LineComment,
        BlockComment,
        DollarQuote(String), // tag including the dollars, e.g. "$$" or "$plpgsql$"
    }

    let mut state = State::Normal;

    while i < bytes.len() {
        match &state {
            State::Normal => {
                let b = bytes[i];
                // Try to recognize a dollar-quote tag opener: $...$
                if b == b'$' {
                    if let Some(tag_end) = bytes[i + 1..].iter().position(|&c| c == b'$') {
                        let tag_body = &bytes[i + 1..i + 1 + tag_end];
                        let valid_tag = tag_body
                            .iter()
                            .all(|&c| c.is_ascii_alphanumeric() || c == b'_');
                        if valid_tag {
                            let full_tag =
                                String::from_utf8_lossy(&bytes[i..=i + 1 + tag_end]).into_owned();
                            buf.push_str(&full_tag);
                            i += full_tag.len();
                            state = State::DollarQuote(full_tag);
                            continue;
                        }
                    }
                    buf.push(b as char);
                    i += 1;
                } else if b == b'\'' {
                    buf.push('\'');
                    i += 1;
                    state = State::SingleQuote;
                } else if b == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
                    buf.push_str("--");
                    i += 2;
                    state = State::LineComment;
                } else if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                    buf.push_str("/*");
                    i += 2;
                    state = State::BlockComment;
                } else if b == b';' {
                    out.push(mem::take(&mut buf));
                    i += 1;
                } else {
                    buf.push(b as char);
                    i += 1;
                }
            }
            State::SingleQuote => {
                let b = bytes[i];
                buf.push(b as char);
                i += 1;
                if b == b'\'' {
                    if i < bytes.len() && bytes[i] == b'\'' {
                        buf.push('\'');
                        i += 1;
                    } else {
                        state = State::Normal;
                    }
                }
            }
            State::LineComment => {
                let b = bytes[i];
                buf.push(b as char);
                i += 1;
                if b == b'\n' {
                    state = State::Normal;
                }
            }
            State::BlockComment => {
                let b = bytes[i];
                buf.push(b as char);
                i += 1;
                if b == b'*' && i < bytes.len() && bytes[i] == b'/' {
                    buf.push('/');
                    i += 1;
                    state = State::Normal;
                }
            }
            State::DollarQuote(tag) => {
                if bytes[i..].starts_with(tag.as_bytes()) {
                    buf.push_str(tag);
                    i += tag.len();
                    state = State::Normal;
                } else {
                    buf.push(bytes[i] as char);
                    i += 1;
                }
            }
        }
    }

    if !buf.trim().is_empty() {
        out.push(buf);
    }
    out
}

#[cfg(test)]
mod migration_list_tests {
    use super::{core_migrations, RETIRED_MIGRATIONS};

    #[test]
    fn a_retired_migration_is_never_applied_and_its_replacement_is() {
        let ids: Vec<&str> = core_migrations().iter().map(|(id, _)| *id).collect();
        for (retired, replaced_by) in RETIRED_MIGRATIONS {
            assert!(
                !ids.contains(retired),
                "{retired} is retired but still listed"
            );
            assert!(
                ids.contains(replaced_by),
                "{retired}'s replacement {replaced_by} is not listed"
            );
        }
    }

    #[test]
    fn migration_ids_are_unique_and_ascending() {
        let ids: Vec<&str> = core_migrations().iter().map(|(id, _)| *id).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(ids, sorted);
    }

    /// No applied migration creates or alters a `fn_*` table: those belong
    /// to Java (`fn_`) or Go (`fn_`, later `fng_`) on a shared database.
    #[test]
    fn no_applied_migration_touches_a_fn_table() {
        let fn_table = regex::Regex::new(r"(?i)\b(idx_)?fn_[a-z]").unwrap();
        for (id, sql) in core_migrations() {
            let code: String = sql
                .lines()
                .filter(|l| !l.trim_start().starts_with("--"))
                .collect::<Vec<_>>()
                .join("\n");
            assert!(!fn_table.is_match(&code), "{id} names a fn_ table");
        }
    }
}

#[cfg(test)]
mod sql_split_tests {
    use super::split_sql_statements;

    #[test]
    fn splits_simple_statements() {
        let sql = "SELECT 1; SELECT 2;";
        let parts = split_sql_statements(sql);
        assert_eq!(parts.len(), 2);
    }

    #[test]
    fn preserves_dollar_quoted_block() {
        let sql = "DO $$ BEGIN SELECT 1; SELECT 2; END $$; SELECT 3;";
        let parts = split_sql_statements(sql);
        assert_eq!(parts.len(), 2);
        assert!(parts[0].contains("BEGIN"));
        assert!(parts[0].contains("END"));
    }

    #[test]
    fn handles_tagged_dollar_quote() {
        let sql = "CREATE FUNCTION f() RETURNS void AS $body$ BEGIN END; $body$ LANGUAGE plpgsql; SELECT 1;";
        let parts = split_sql_statements(sql);
        assert_eq!(parts.len(), 2);
    }

    #[test]
    fn ignores_semicolons_in_strings() {
        let sql = "INSERT INTO t VALUES ('a;b'); SELECT 1;";
        let parts = split_sql_statements(sql);
        assert_eq!(parts.len(), 2);
    }
}
