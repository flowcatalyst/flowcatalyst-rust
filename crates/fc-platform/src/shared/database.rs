//! PostgreSQL Database Connection (SQLx)
//!
//! Provides:
//! - `PgPool` creation with shared env-driven pool config.
//! - `SecretProvider` abstraction (env / AWS Secrets Manager) and a background
//!   refresh task that polls the provider on an interval and updates the pool's
//!   connection options when the DB password rotates. Existing repositories do
//!   not need to change — `PgPool::set_connect_options` mutates the pool in
//!   place, so any future connection (including reconnects after `max_lifetime`)
//!   uses the new credentials.
//!
//! This mirrors the TS `flowcatalyst` approach (timer-based polling + graceful
//! refresh) but takes advantage of sqlx's in-place options update so we don't
//! need to swap pool handles or refactor every repository.

use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

// ── Pool config ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct PoolConfig {
    max_connections: u32,
    min_connections: u32,
    connect_timeout: u64,
    idle_timeout: u64,
    max_lifetime: u64,
}

impl PoolConfig {
    fn from_env() -> Self {
        Self {
            max_connections: env_parse("FC_DB_MAX_CONNECTIONS", 10),
            min_connections: env_parse("FC_DB_MIN_CONNECTIONS", 2),
            connect_timeout: env_parse("FC_DB_CONNECT_TIMEOUT_SECS", 10),
            idle_timeout: env_parse("FC_DB_IDLE_TIMEOUT_SECS", 300),
            max_lifetime: env_parse("FC_DB_MAX_LIFETIME_SECS", 1800),
        }
    }
}

fn env_parse<T: FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Create a new SQLx PgPool with connection pooling.
///
/// Environment-configurable pool settings:
/// * `FC_DB_MAX_CONNECTIONS` (default: 10)
/// * `FC_DB_MIN_CONNECTIONS` (default: 2)
/// * `FC_DB_CONNECT_TIMEOUT_SECS` (default: 10)
/// * `FC_DB_IDLE_TIMEOUT_SECS` (default: 300)
/// * `FC_DB_MAX_LIFETIME_SECS` (default: 1800)
pub async fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    let cfg = PoolConfig::from_env();
    info!(
        max_connections = cfg.max_connections,
        min_connections = cfg.min_connections,
        "Creating SQLx PgPool"
    );

    let pool = PgPoolOptions::new()
        .max_connections(cfg.max_connections)
        .min_connections(cfg.min_connections)
        .acquire_timeout(Duration::from_secs(cfg.connect_timeout))
        .idle_timeout(Duration::from_secs(cfg.idle_timeout))
        .max_lifetime(Duration::from_secs(cfg.max_lifetime))
        .connect(database_url)
        .await?;

    info!("SQLx PgPool established");
    Ok(pool)
}

// ── Secret provider ──────────────────────────────────────────────────────────

/// A source for the database connection URL. Implementations are async because
/// cloud providers (Secrets Manager, GCP Secret Manager) require network calls.
#[async_trait::async_trait]
pub trait SecretProvider: Send + Sync {
    fn name(&self) -> &'static str;
    async fn get_db_url(&self) -> Result<String, anyhow::Error>;
}

/// Where the platform database connection comes from, with Go's precedence
/// (flowcatalyst-go `internal/server/envcfg.go` `ResolveDatabaseURL` and
/// `dbsecret.go`): a full URL beats Secrets Manager, which beats explicit
/// `DB_*` credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatabaseSource {
    /// A ready connection string.
    Url(String),
    /// Username/password (and optionally port) from an RDS-style Secrets
    /// Manager secret; host and database name from the environment.
    AwsSecretsManager {
        secret_arn: String,
        host: String,
        db_name: String,
        /// `DB_PORT`, or 5432; the secret's own `port` wins over it.
        fallback_port: String,
    },
}

/// The local-development default Go falls back to when nothing is configured.
pub const DEFAULT_DATABASE_URL: &str = "postgresql://postgres@localhost:5432/flowcatalyst";

/// [`database_source`] over the process environment.
pub fn database_source_from_env() -> Result<DatabaseSource, anyhow::Error> {
    database_source(|k| std::env::var(k).ok())
}

/// Resolve the database source from `get` (an environment lookup):
/// 1. `FC_DATABASE_URL` / `DATABASE_URL` — used as is;
/// 2. `DB_HOST` + `DB_SECRET_ARN` — Secrets Manager (`DB_SECRET_PROVIDER`
///    must be `aws`, the default; anything else is a startup error, as in
///    Go), with `DB_NAME` (default `flowcatalyst`) and `DB_PORT`;
/// 3. `DB_HOST` + `DB_USERNAME` (default `postgres`) + `DB_PASSWORD`;
/// 4. nothing — [`DEFAULT_DATABASE_URL`].
///
/// Blank values count as unset.
pub fn database_source(
    get: impl Fn(&str) -> Option<String>,
) -> Result<DatabaseSource, anyhow::Error> {
    let get = |k: &str| get(k).filter(|v| !v.trim().is_empty());
    if let Some(url) = get("FC_DATABASE_URL").or_else(|| get("DATABASE_URL")) {
        return Ok(DatabaseSource::Url(url));
    }
    let Some(host) = get("DB_HOST") else {
        return Ok(DatabaseSource::Url(DEFAULT_DATABASE_URL.to_string()));
    };
    let db_name = get("DB_NAME").unwrap_or_else(|| "flowcatalyst".to_string());
    let port = get("DB_PORT").unwrap_or_else(|| "5432".to_string());

    if let Some(secret_arn) = get("DB_SECRET_ARN") {
        let provider = get("DB_SECRET_PROVIDER").unwrap_or_else(|| "aws".to_string());
        if !provider.eq_ignore_ascii_case("aws") {
            anyhow::bail!("DB_SECRET_PROVIDER {provider:?} not supported (only \"aws\")");
        }
        return Ok(DatabaseSource::AwsSecretsManager {
            secret_arn,
            host,
            db_name,
            fallback_port: port,
        });
    }

    let username = get("DB_USERNAME").unwrap_or_else(|| "postgres".to_string());
    let host_port = if host.contains(':') {
        host
    } else {
        format!("{host}:{port}")
    };
    Ok(DatabaseSource::Url(match get("DB_PASSWORD") {
        None => format!("postgresql://{username}@{host_port}/{db_name}"),
        Some(password) => format!(
            "postgresql://{username}:{}@{host_port}/{db_name}",
            urlencoding::encode(&password)
        ),
    }))
}

/// The credential-rotation poll interval: `DB_SECRET_REFRESH_INTERVAL_MS`,
/// 5 minutes when unset or unparseable; zero or negative disables polling
/// (Go `NewDBSecretRefresher`).
pub fn secret_refresh_interval_from_env() -> Duration {
    let ms = std::env::var("DB_SECRET_REFRESH_INTERVAL_MS")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(300_000);
    if ms <= 0 {
        Duration::ZERO
    } else {
        Duration::from_millis(ms as u64)
    }
}

/// The region of an ARN (`arn:partition:service:REGION:account:resource`),
/// or `None` for a bare secret name (Go `regionFromARN`).
pub fn region_from_arn(arn: &str) -> Option<String> {
    let parts: Vec<&str> = arn.split(':').collect();
    (parts.len() >= 4 && parts[0] == "arn" && !parts[3].is_empty()).then(|| parts[3].to_string())
}

/// Build the connection URL from an RDS-style secret's JSON
/// (`{"username","password","port"?}`): the secret's port beats
/// `fallback_port`, a host that already names a port keeps it, and the
/// password is percent-encoded (Go `buildDBSecretDSN`).
pub fn db_url_from_secret_json(
    secret_json: &str,
    host: &str,
    db_name: &str,
    fallback_port: &str,
) -> Result<String, anyhow::Error> {
    let creds: serde_json::Value = serde_json::from_str(secret_json)
        .map_err(|e| anyhow::anyhow!("Failed to parse DB secret JSON: {}", e))?;
    let field = |k: &str| creds[k].as_str().filter(|v| !v.is_empty());
    let (Some(username), Some(password)) = (field("username"), field("password")) else {
        anyhow::bail!("DB secret is missing username/password");
    };
    let port = creds["port"]
        .as_u64()
        .filter(|p| *p > 0)
        .map(|p| p.to_string())
        .unwrap_or_else(|| fallback_port.to_string());
    let host_port = if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:{port}")
    };
    Ok(format!(
        "postgresql://{}:{}@{}/{}",
        username,
        urlencoding::encode(password),
        host_port,
        db_name
    ))
}

/// AWS Secrets Manager provider. Reads `{"username":..., "password":..., "port":...}`
/// JSON from a secret and constructs a `postgresql://` URL using the supplied
/// host and database name.
///
/// The client's region is the secret ARN's own (a secret must be read from
/// its region, and ECS on EC2 need not export `AWS_REGION`), as in Go.
/// Credentials come from the default chain (the ECS task role in
/// production). The SDK's endpoint override (`AWS_ENDPOINT_URL_SECRETS_MANAGER`
/// / `AWS_ENDPOINT_URL`) is honoured, which is how tests point it at a fake.
pub struct AwsSecretProvider {
    secret_arn: String,
    host: String,
    db_name: String,
    fallback_port: String,
    client: tokio::sync::OnceCell<aws_sdk_secretsmanager::Client>,
}

impl AwsSecretProvider {
    pub fn new(secret_arn: String, host: String, db_name: String, fallback_port: String) -> Self {
        Self {
            secret_arn,
            host,
            db_name,
            fallback_port,
            client: tokio::sync::OnceCell::new(),
        }
    }

    async fn client(&self) -> &aws_sdk_secretsmanager::Client {
        self.client
            .get_or_init(|| async {
                let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest());
                if let Some(region) = region_from_arn(&self.secret_arn) {
                    loader = loader.region(aws_config::Region::new(region));
                }
                aws_sdk_secretsmanager::Client::new(&loader.load().await)
            })
            .await
    }
}

#[async_trait::async_trait]
impl SecretProvider for AwsSecretProvider {
    fn name(&self) -> &'static str {
        "aws-secrets-manager"
    }

    async fn get_db_url(&self) -> Result<String, anyhow::Error> {
        let secret = self
            .client()
            .await
            .get_secret_value()
            .secret_id(&self.secret_arn)
            .send()
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to get DB secret from Secrets Manager: {}",
                    aws_sdk_secretsmanager::error::DisplayErrorContext(&e)
                )
            })?;

        let secret_string = secret
            .secret_string()
            .ok_or_else(|| anyhow::anyhow!("DB secret has no string value"))?;

        db_url_from_secret_json(
            secret_string,
            &self.host,
            &self.db_name,
            &self.fallback_port,
        )
    }
}

// ── Background refresh task ──────────────────────────────────────────────────

/// Spawn a background task that polls `provider` on `interval` and, when the
/// resolved DB URL changes, updates the connection options on the pool.
///
/// Mirrors the TypeScript flowcatalyst approach (timer-based polling + graceful
/// refresh). Takes advantage of AWS RDS's dual-password rotation window: both
/// old and new passwords are valid for a period after rotation, so a periodic
/// poll catches the change before the old password is invalidated.
///
/// Disable by passing `Duration::ZERO` for `interval`.
pub fn start_secret_refresh(
    provider: Arc<dyn SecretProvider>,
    pg_pool: PgPool,
    initial_url: String,
    interval: Duration,
) {
    if interval.is_zero() {
        info!("DB secret refresh disabled (interval=0)");
        return;
    }
    info!(
        provider = provider.name(),
        interval_secs = interval.as_secs(),
        "Starting DB secret refresh task"
    );
    tokio::spawn(async move {
        let mut current_url = initial_url;
        loop {
            tokio::time::sleep(interval).await;
            match provider.get_db_url().await {
                Ok(new_url) => {
                    if new_url == current_url {
                        continue;
                    }
                    info!(
                        provider = provider.name(),
                        "DB credentials changed — updating pool connect options"
                    );
                    match PgConnectOptions::from_str(&new_url) {
                        Ok(opts) => {
                            // New connections (and reconnects after `max_lifetime`)
                            // will use the new credentials. The dual-password
                            // window on RDS keeps existing connections valid
                            // until they cycle out naturally.
                            pg_pool.set_connect_options(opts);
                            current_url = new_url;
                            info!("Pool connect options updated successfully");
                        }
                        Err(e) => {
                            error!(error = %e, "Failed to parse refreshed DB URL");
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        provider = provider.name(),
                        error = %e,
                        "Failed to poll secret provider for credential changes"
                    );
                }
            }
        }
    });
}

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
pub async fn run_migrations(pool: &PgPool, profile: MigrationProfile) -> Result<(), sqlx::Error> {
    info!(?profile, "Running database migrations...");

    // Migrations applied to every profile.
    let core_migrations: &[(&str, &str)] = &[
        (
            "001_tenant_tables",
            include_str!("../../../../migrations/001_tenant_tables.sql"),
        ),
        (
            "002_iam_tables",
            include_str!("../../../../migrations/002_iam_tables.sql"),
        ),
        (
            "003_application_tables",
            include_str!("../../../../migrations/003_application_tables.sql"),
        ),
        (
            "004_messaging_tables",
            include_str!("../../../../migrations/004_messaging_tables.sql"),
        ),
        (
            "005_outbox_tables",
            include_str!("../../../../migrations/005_outbox_tables.sql"),
        ),
        (
            "006_audit_tables",
            include_str!("../../../../migrations/006_audit_tables.sql"),
        ),
        (
            "007_oauth_tables",
            include_str!("../../../../migrations/007_oauth_tables.sql"),
        ),
        (
            "008_auth_tracking_tables",
            include_str!("../../../../migrations/008_auth_tracking_tables.sql"),
        ),
        (
            "009_p0_alignment",
            include_str!("../../../../migrations/009_p0_alignment.sql"),
        ),
        (
            "010_auth_state_tables",
            include_str!("../../../../migrations/010_auth_state_tables.sql"),
        ),
        (
            "011_dispatch_job_tables",
            include_str!("../../../../migrations/011_dispatch_job_tables.sql"),
        ),
        (
            "012_projection_columns",
            include_str!("../../../../migrations/012_projection_columns.sql"),
        ),
        (
            "013_drop_connection_endpoint",
            include_str!("../../../../migrations/013_drop_connection_endpoint.sql"),
        ),
        (
            "014_widen_attempt_type",
            include_str!("../../../../migrations/014_widen_attempt_type.sql"),
        ),
        (
            "015_dispatch_jobs_write_indexes",
            include_str!("../../../../migrations/015_dispatch_jobs_write_indexes.sql"),
        ),
        (
            "016_clean_orphaned_role_assignments",
            include_str!("../../../../migrations/016_clean_orphaned_role_assignments.sql"),
        ),
        (
            "017_dispatch_pool_rate_limit_nullable",
            include_str!("../../../../migrations/017_dispatch_pool_rate_limit_nullable.sql"),
        ),
        // 018 reshapes the messaging tables into the partitioning-ready
        // schema (composite PKs, fanned_out_at, read-table created_at).
        (
            "018_partition_prep",
            include_str!("../../../../migrations/018_partition_prep.sql"),
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
            include_str!("../../../../migrations/019_partition_messaging_tables.sql"),
        ),
        (
            "020_webauthn_credentials",
            include_str!("../../../../migrations/020_webauthn_credentials.sql"),
        ),
        (
            "021_scheduled_jobs",
            include_str!("../../../../migrations/021_scheduled_jobs.sql"),
        ),
        (
            "022_partition_scheduled_job_history",
            include_str!("../../../../migrations/022_partition_scheduled_job_history.sql"),
        ),
        // Bridges DBs that ran 021 before `target_url` was added to it.
        (
            "024_scheduled_jobs_add_target_url",
            include_str!("../../../../migrations/024_scheduled_jobs_add_target_url.sql"),
        ),
        (
            "025_application_openapi_specs",
            include_str!("../../../../migrations/025_application_openapi_specs.sql"),
        ),
        (
            "026_processes",
            include_str!("../../../../migrations/026_processes.sql"),
        ),
        // Wires the FK from oauth_clients.service_account_principal_id back
        // to iam_principals (CASCADE), and deletes orphaned clients left
        // behind by SA deletes that pre-dated this constraint. Idempotent:
        // drops the constraint before re-adding.
        (
            "027_oauth_clients_service_account_fk",
            include_str!("../../../../migrations/027_oauth_clients_service_account_fk.sql"),
        ),
        // Wires the FK from app_applications.service_account_id back to
        // iam_principals (SET NULL so the application survives SA delete),
        // and clears any dangling references so a replacement SA can be
        // provisioned.
        (
            "028_application_service_account_fk",
            include_str!("../../../../migrations/028_application_service_account_fk.sql"),
        ),
        (
            "029_oauth_client_post_logout_redirect_uris",
            include_str!("../../../../migrations/029_oauth_client_post_logout_redirect_uris.sql"),
        ),
        (
            "030_rate_limit_events",
            include_str!("../../../../migrations/030_rate_limit_events.sql"),
        ),
        // Go's 034: the stored all-applications flag on principals.
        (
            "031_principal_all_applications",
            include_str!("../../../../migrations/031_principal_all_applications.sql"),
        ),
        // The iam_service_accounts columns of Go's 035: the requested scope
        // and the client links, stored on the service account.
        (
            "032_service_account_scope_and_client_ids",
            include_str!("../../../../migrations/032_service_account_scope_and_client_ids.sql"),
        ),
        // Go's 046 + 047: the OAuth client secret-rotation overlap.
        (
            "033_oauth_client_secret_grace",
            include_str!("../../../../migrations/033_oauth_client_secret_grace.sql"),
        ),
        // Java's V13 + V15 - V16: the function registry's fn_* tables, and
        // msg_subscriptions.source gaining FUNCTION.
        (
            "034_functions",
            include_str!("../../../../migrations/034_functions.sql"),
        ),
        // Java's msg_scheduled_jobs.application_id: a function's schedules
        // are signed with its application's credentials.
        (
            "035_scheduled_jobs_application_id",
            include_str!("../../../../migrations/035_scheduled_jobs_application_id.sql"),
        ),
        // 036 is a data migration in Rust, not SQL, run after these:
        // `036_scheduled_job_cron_dialect` (see below).
        // Owner decision 5: `runtime: component`, and the runtimes a host
        // reports in its heartbeat.
        (
            "037_function_component_runtime",
            include_str!("../../../../migrations/037_function_component_runtime.sql"),
        ),
        // Java's V18: aud_logs.entity_id widened to 100, so a sync rollup's
        // audit row (keyed by the application code) fits.
        (
            "038_aud_logs_entity_id_width",
            include_str!("../../../../migrations/038_aud_logs_entity_id_width.sql"),
        ),
        // Go's 054 + 057 (the parts the dispatch pipeline needs): a job's
        // own queue priority, and what an attempt sent.
        (
            "039_dispatch_job_queue_and_attempt_request",
            include_str!("../../../../migrations/039_dispatch_job_queue_and_attempt_request.sql"),
        ),
        // Go's 031: two-factor authentication (factors, recovery codes,
        // email PINs, trusted devices, the per-domain policy).
        (
            "041_mfa_tables",
            include_str!("../../../../migrations/041_mfa_tables.sql"),
        ),
        // The reset-token columns of Go's 031, 032, 033, 041 and 051.
        (
            "042_password_reset_token_purpose",
            include_str!("../../../../migrations/042_password_reset_token_purpose.sql"),
        ),
        // Go's 042: the OAuth client flag that makes an interactive login's
        // access token authority-bearing (owner decisions #3/#20).
        (
            "043_oauth_client_api_access",
            include_str!("../../../../migrations/043_oauth_client_api_access.sql"),
        ),
        // Go's 032: the lost-device reset approval queue.
        (
            "044_reset_approval_requests",
            include_str!("../../../../migrations/044_reset_approval_requests.sql"),
        ),
        // Go's 041 + 043: the portal identity plane (portal_identities,
        // portal_login_flows, the portal flags on OAuth clients and OIDC
        // login states, the reset-token redirect).
        (
            "046_portal_identities",
            include_str!("../../../../migrations/046_portal_identities.sql"),
        ),
        // Go's 053: portal apps and per-app grants.
        (
            "047_portal_apps",
            include_str!("../../../../migrations/047_portal_apps.sql"),
        ),
        // Go's 056: application-scoped connections (application_code,
        // source) and the (application_code, client_id, code) uniqueness.
        (
            "050_connection_application_scope",
            include_str!("../../../../migrations/050_connection_application_scope.sql"),
        ),
        // Go's 044: application-synced documentation.
        (
            "051_app_docs",
            include_str!("../../../../migrations/051_app_docs.sql"),
        ),
        // Go's 039: the self-service developer API credential.
        (
            "052_developer_api_credentials",
            include_str!("../../../../migrations/052_developer_api_credentials.sql"),
        ),
    ];

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
        // A database Java migrated to V16 has all of 034's effects: the last
        // table, V15's column, V16's dropped column and the widened source
        // CHECK.
        (
            "034_functions",
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_name = 'fn_secrets') \
             AND EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'fn_routes' \
               AND column_name = 'alias_prefixes') \
             AND NOT EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'fn_domains' \
               AND column_name = 'verification_token') \
             AND EXISTS (SELECT 1 FROM pg_constraint \
             WHERE conname = 'chk_msg_subscriptions_source' \
               AND pg_get_constraintdef(oid) LIKE '%FUNCTION%')",
        ),
        // A database Java migrated has the column from its baseline.
        (
            "035_scheduled_jobs_application_id",
            "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' \
               AND table_name = 'msg_scheduled_jobs' \
               AND column_name = 'application_id')",
        ),
        // Both effects: the widened CHECK and the hosts' runtimes column.
        (
            "037_function_component_runtime",
            "SELECT EXISTS (SELECT 1 FROM pg_constraint \
             WHERE conname = 'fn_functions_runtime_check' \
               AND pg_get_constraintdef(oid) LIKE '%COMPONENT%') \
             AND EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'fn_hosts' \
               AND column_name = 'runtimes')",
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

    // Apply each migration if not already tracked.
    for (id, sql) in core_migrations.iter() {
        apply_tracked(pool, id, sql).await?;
    }
    if profile == MigrationProfile::Production {
        for (id, sql) in production_migrations.iter() {
            apply_tracked(pool, id, sql).await?;
        }
    }

    // 036: the one-off rewrite of crons the previous Rust poller read in the
    // `cron` crate's dialect into Java/Go's. It needs the old parser, so it
    // is Rust; it tracks itself in `_schema_migrations` (never on a
    // database another platform has migrated: see the module docs). It is
    // not in the pre-tracker backfill above on purpose: a pre-tracker Rust
    // database still needs it, and its own guard covers Go's.
    crate::scheduled_job::cron_migration::run(pool).await?;

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
    let start = std::time::Instant::now();
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
pub(crate) fn sha256_hex(content: &str) -> String {
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
                    out.push(std::mem::take(&mut buf));
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
pub async fn seed_platform_application(pool: &PgPool) -> crate::shared::error::Result<()> {
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
pub async fn seed_platform_event_types(pool: &PgPool) -> crate::shared::error::Result<()> {
    let repo = crate::event_type::repository::EventTypeRepository::new(pool);
    let defs = crate::seed::platform_event_types::definitions();
    let inserted = repo.seed_catalogue(&defs).await?;
    if inserted > 0 {
        info!(inserted, total = defs.len(), "Seeded platform event types");
    }
    Ok(())
}

pub async fn seed_builtin_roles(pool: &PgPool) -> crate::shared::error::Result<()> {
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

#[cfg(test)]
mod database_source_tests {
    use super::*;
    use std::collections::HashMap;

    fn source(vars: &[(&str, &str)]) -> Result<DatabaseSource, anyhow::Error> {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        database_source(|k| map.get(k).cloned())
    }

    #[test]
    fn a_full_url_beats_everything() {
        let s = source(&[
            ("DATABASE_URL", "postgresql://a@b/c"),
            ("DB_HOST", "h"),
            ("DB_SECRET_ARN", "arn"),
        ])
        .unwrap();
        assert_eq!(s, DatabaseSource::Url("postgresql://a@b/c".into()));
    }

    #[test]
    fn the_production_task_env_resolves_to_secrets_manager() {
        let s = source(&[
            ("DB_SECRET_PROVIDER", "aws"),
            (
                "DB_SECRET_ARN",
                "arn:aws:secretsmanager:eu-west-1:1:secret:rds!db-x",
            ),
            ("DB_HOST", "db.example.internal"),
            ("DB_NAME", "flowcatalyst"),
        ])
        .unwrap();
        assert_eq!(
            s,
            DatabaseSource::AwsSecretsManager {
                secret_arn: "arn:aws:secretsmanager:eu-west-1:1:secret:rds!db-x".into(),
                host: "db.example.internal".into(),
                db_name: "flowcatalyst".into(),
                fallback_port: "5432".into(),
            }
        );
    }

    #[test]
    fn an_unknown_secret_provider_is_refused() {
        let err = source(&[
            ("DB_SECRET_PROVIDER", "gcp"),
            ("DB_SECRET_ARN", "x"),
            ("DB_HOST", "h"),
        ])
        .unwrap_err();
        assert!(err.to_string().contains("DB_SECRET_PROVIDER"));
    }

    #[test]
    fn explicit_credentials_and_the_local_default() {
        assert_eq!(
            source(&[("DB_HOST", "h"), ("DB_PASSWORD", "p w/@")]).unwrap(),
            DatabaseSource::Url("postgresql://postgres:p%20w%2F%40@h:5432/flowcatalyst".into())
        );
        assert_eq!(
            source(&[
                ("DB_HOST", "h:6000"),
                ("DB_USERNAME", "u"),
                ("DB_NAME", "n")
            ])
            .unwrap(),
            DatabaseSource::Url("postgresql://u@h:6000/n".into())
        );
        assert_eq!(
            source(&[]).unwrap(),
            DatabaseSource::Url(DEFAULT_DATABASE_URL.into())
        );
    }

    #[test]
    fn the_secret_port_beats_db_port_and_passwords_are_escaped() {
        let url = db_url_from_secret_json(
            r#"{"username":"admin","password":"a:b@c","port":6543}"#,
            "db",
            "fc",
            "5432",
        )
        .unwrap();
        assert_eq!(url, "postgresql://admin:a%3Ab%40c@db:6543/fc");
        let url = db_url_from_secret_json(r#"{"username":"u","password":"p"}"#, "db", "fc", "7000")
            .unwrap();
        assert_eq!(url, "postgresql://u:p@db:7000/fc");
        assert!(
            db_url_from_secret_json(r#"{"username":"u","password":""}"#, "db", "fc", "1").is_err()
        );
    }

    #[test]
    fn the_region_comes_from_the_arn() {
        assert_eq!(
            region_from_arn("arn:aws:secretsmanager:ap-southeast-2:123:secret:x").as_deref(),
            Some("ap-southeast-2")
        );
        assert_eq!(region_from_arn("my-secret-name"), None);
    }
}
