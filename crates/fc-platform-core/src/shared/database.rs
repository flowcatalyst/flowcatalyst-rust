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

use aws_sdk_secretsmanager::error::DisplayErrorContext;
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};
use std::env;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::OnceCell;
use tokio::time;
use tracing::{error, info, warn};

// The migration runner lives in `fc-migrations` (see its docs for why); it is
// part of this module's API as it always was.
pub use fc_migrations::{run_migrations_with, sha256_hex, CodeMigration, MigrationProfile};

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
    env::var(key)
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
    create_pool_with(
        PgConnectOptions::from_str(database_url)?,
        PoolConfig::from_env(),
    )
    .await
}

/// The scheduler's planner settings. Every connection of the dispatch
/// scheduler's pool sets them at connect time, and no other pool does.
///
/// * `plan_cache_mode = force_custom_plan`: sqlx prepares and caches every
///   statement per connection, and after a few executions PostgreSQL may switch
///   a cached statement to a GENERIC plan. A generic plan made while the queue
///   was empty is a seq scan, and is then reused after a burst (measured:
///   850 ms per claim at 200,000 queue rows) until the next autoanalyze.
/// * `enable_sort = off`: the claim's `ORDER BY ... LIMIT` must walk
///   `idx_dispatch_queue_order`, never sort the queue (without statistics, or
///   with statistics taken while the queue was empty, the planner prefers a
///   seq scan plus a sort).
pub const SCHEDULER_PLANNER_OPTIONS: [(&str, &str); 2] = [
    ("plan_cache_mode", "force_custom_plan"),
    ("enable_sort", "off"),
];

/// `opts` with [`SCHEDULER_PLANNER_OPTIONS`] applied at connect time.
pub fn with_scheduler_planner_options(opts: PgConnectOptions) -> PgConnectOptions {
    opts.options(SCHEDULER_PLANNER_OPTIONS)
}

/// The dispatch scheduler's OWN pool: `max_connections` of its own (the other
/// settings still come from the environment), so it does not compete with the
/// API for connections, and [`SCHEDULER_PLANNER_OPTIONS`] on every connection.
/// The minimum is capped at the maximum. Use it for the scheduler and nothing
/// else: the platform's pool must keep the server's planner defaults.
pub async fn create_scheduler_pool(
    database_url: &str,
    max_connections: u32,
) -> Result<PgPool, sqlx::Error> {
    let mut cfg = PoolConfig::from_env();
    cfg.max_connections = max_connections.max(1);
    cfg.min_connections = cfg.min_connections.min(cfg.max_connections);
    let opts = with_scheduler_planner_options(PgConnectOptions::from_str(database_url)?);
    create_pool_with(opts, cfg).await
}

async fn create_pool_with(opts: PgConnectOptions, cfg: PoolConfig) -> Result<PgPool, sqlx::Error> {
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
        .connect_with(opts)
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
    database_source(|k| env::var(k).ok())
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
    let ms = env::var("DB_SECRET_REFRESH_INTERVAL_MS")
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
    client: OnceCell<aws_sdk_secretsmanager::Client>,
}

impl AwsSecretProvider {
    pub fn new(secret_arn: String, host: String, db_name: String, fallback_port: String) -> Self {
        Self {
            secret_arn,
            host,
            db_name,
            fallback_port,
            client: OnceCell::new(),
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
                    DisplayErrorContext(&e)
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
    start_secret_refresh_with(provider, pg_pool, initial_url, interval, |o| o);
}

/// [`start_secret_refresh`] for the scheduler's pool: the refreshed connect
/// options keep [`SCHEDULER_PLANNER_OPTIONS`].
pub fn start_scheduler_secret_refresh(
    provider: Arc<dyn SecretProvider>,
    pg_pool: PgPool,
    initial_url: String,
    interval: Duration,
) {
    start_secret_refresh_with(
        provider,
        pg_pool,
        initial_url,
        interval,
        with_scheduler_planner_options,
    );
}

fn start_secret_refresh_with(
    provider: Arc<dyn SecretProvider>,
    pg_pool: PgPool,
    initial_url: String,
    interval: Duration,
    tune: fn(PgConnectOptions) -> PgConnectOptions,
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
            time::sleep(interval).await;
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
                            pg_pool.set_connect_options(tune(opts));
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
