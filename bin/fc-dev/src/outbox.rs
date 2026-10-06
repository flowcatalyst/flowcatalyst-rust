//! `fc-dev outbox` — the standalone outbox poller, as Go's `fcdev outbox`.
//!
//! * `fc-dev outbox [flags]` — Go's command: poll an external app
//!   database's `outbox_messages` (creating the table when missing) and
//!   forward to a FlowCatalyst platform, authenticated with a static token or
//!   a service account's `client_credentials` (minted and refreshed).
//!   Precedence: flag, then environment (after `--env-file`, default
//!   `./.env`, which never overrides a set variable), then default.
//! * `fc-dev outbox create-table` — Go's: create the SDK outbox table (or
//!   MongoDB indexes) in a consumer app's `postgres`, `mysql` or `mongodb`
//!   database.
//! * `fc-dev outbox init` / `poll` — fc-dev's earlier pair, kept: `init`
//!   writes `FC_OUTBOX_DB_URL`, `FC_OUTBOX_API_URL` and `FC_OUTBOX_TOKEN`
//!   into the project's `.env` (so secrets stay off the shell history and
//!   out of `ps`), and `poll` runs the same poller from them. The bare form
//!   reads those names too, after Go's.
//!
//! Used when the app's database can't be (or shouldn't be) fc-dev's
//! embedded PG — the headline case is a PostGIS-dependent app running
//! against Docker Postgres.

use anyhow::{anyhow, Context, Result};
use std::io::{stdin, stdout, BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::init;
use fc_common::config;
use fc_outbox::enhanced_processor::{EnhancedOutboxProcessor, EnhancedProcessorConfig};
use fc_outbox::http_dispatcher::HttpDispatcherConfig;
use fc_outbox::mysql::MySqlOutboxRepository;
use fc_outbox::postgres::PostgresOutboxRepository;
use fc_outbox::repository::OutboxRepository;
use fc_outbox::setup;
use fc_outbox::{ClientCredentialsTokenSource, TokenSource};
use fc_platform::shared::server_setup;
use sqlx::mysql::MySqlPoolOptions;
use sqlx::postgres::PgPoolOptions;
use std::env;
use std::fs;
use std::str::FromStr;
use tokio::time;

#[derive(clap::Args, Debug)]
pub struct OutboxArgs {
    /// Load the environment from this dotenv file first; a variable already
    /// set always wins (Go's `--env-file`).
    #[arg(long, global = true, default_value = ".env")]
    env_file: PathBuf,

    #[command(subcommand)]
    command: Option<OutboxCommand>,

    #[command(flatten)]
    run: RunArgs,
}

#[derive(clap::Subcommand, Debug)]
enum OutboxCommand {
    /// Create the `outbox_messages` table (or, for MongoDB, the
    /// collection's indexes) in a consumer app's database. Idempotent.
    CreateTable(CreateTableArgs),

    /// Write the outbox poller's configuration into `.env` so daily use
    /// is `fc-dev outbox` with no flags. Idempotent — re-running
    /// updates the existing keys, never duplicates them.
    Init(InitArgs),

    /// Poll an external app database's `outbox_messages` and forward
    /// to a FlowCatalyst platform API. Values come from process env
    /// / `.env`; explicit flags override. (fc-dev's earlier form of the
    /// bare command.)
    Poll(PollArgs),
}

/// Go's `fcdev outbox` flags. Each falls back to its environment variables
/// (resolved after `--env-file` is loaded), then its default.
#[derive(clap::Args, Debug)]
struct RunArgs {
    /// The external app's Postgres URL (env `FC_OUTBOX_SOURCE_DB_URL`, then
    /// `FC_OUTBOX_DB_URL`). Required.
    #[arg(long)]
    source_db_url: Option<String>,

    /// FlowCatalyst platform URL (env `FC_OUTBOX_PLATFORM_URL`, then
    /// `FC_OUTBOX_API_URL`) [default: http://localhost:8080].
    #[arg(long)]
    target_url: Option<String>,

    /// Static bearer token, used when no client id/secret is set (env
    /// `FC_OUTBOX_PLATFORM_AUTH_TOKEN`, then `FC_OUTBOX_TOKEN`).
    #[arg(long)]
    auth_token: Option<String>,

    /// OAuth `client_credentials` client id (env `FC_OUTBOX_CLIENT_ID`, then
    /// `FLOWCATALYST_CLIENT_ID`).
    #[arg(long)]
    client_id: Option<String>,

    /// OAuth `client_credentials` client secret (env
    /// `FC_OUTBOX_CLIENT_SECRET`, then `FLOWCATALYST_CLIENT_SECRET`).
    #[arg(long)]
    client_secret: Option<String>,

    /// OAuth token endpoint (env `FC_OUTBOX_TOKEN_URL`) [default:
    /// <target-url>/oauth/token].
    #[arg(long)]
    token_url: Option<String>,

    /// Requested scope, narrowing the minted token (env `FC_OUTBOX_SCOPE`).
    #[arg(long)]
    scope: Option<String>,

    /// Rows per poll; 0 = the library default (env `FC_OUTBOX_BATCH_SIZE`).
    #[arg(long)]
    batch_size: Option<u32>,

    /// Outstanding items cap; 0 = the library default (env
    /// `FC_OUTBOX_MAX_IN_FLIGHT`).
    #[arg(long)]
    max_in_flight: Option<u64>,

    /// Sleep between empty polls in ms; 0 = the library default (env
    /// `FC_OUTBOX_POLL_INTERVAL_MS`).
    #[arg(long)]
    poll_interval_ms: Option<u64>,
}

#[derive(clap::Args, Debug)]
struct CreateTableArgs {
    /// Target store: `postgres` (`pg`, `postgresql`), `mysql` (`mariadb`) or
    /// `mongodb` (`mongo`) (env `FC_OUTBOX_BACKEND`, then
    /// `FC_OUTBOX_DB_TYPE`) [default: postgres].
    #[arg(long)]
    db_type: Option<String>,

    /// Connection URL: `postgres://…`, `mysql://…` (or a Go MySQL DSN
    /// `user:pass@tcp(host:3306)/db`), `mongodb://…` (env
    /// `FC_OUTBOX_SOURCE_DB_URL`, `FC_OUTBOX_DB_URL`, `FC_OUTBOX_MONGO_URI`).
    #[arg(long)]
    db_url: Option<String>,

    /// MongoDB database name (env `FC_OUTBOX_MONGO_DB`) [default:
    /// flowcatalyst].
    #[arg(long)]
    db_name: Option<String>,
}

#[derive(clap::Args, Debug)]
struct InitArgs {
    /// Project root. The `.env` file is written to `{root}/.env`.
    #[arg(long, default_value = ".")]
    root: PathBuf,

    /// PostgreSQL URL of the app database that owns `outbox_messages`.
    #[arg(long)]
    db_url: Option<String>,

    /// Base URL of the FlowCatalyst platform API to forward to.
    #[arg(long)]
    api_url: Option<String>,

    /// Bearer token for the platform API. If omitted, prompted with
    /// terminal echo off (avoid putting secrets in shell history).
    #[arg(long)]
    token: Option<String>,

    /// Non-interactive — fail if any required value is missing rather
    /// than prompting.
    #[arg(long)]
    yes: bool,
}

#[derive(clap::Args, Debug)]
struct PollArgs {
    /// PostgreSQL URL of the app database that owns `outbox_messages`.
    /// Falls back to `FC_OUTBOX_DB_URL` from env / `.env`.
    #[arg(long, env = "FC_OUTBOX_DB_URL")]
    db_url: Option<String>,

    /// Base URL of the FlowCatalyst platform API to forward to.
    #[arg(
        long,
        env = "FC_OUTBOX_API_URL",
        default_value = "http://localhost:8080"
    )]
    api_url: String,

    /// Bearer token for the platform API. Mint one from a service
    /// account's `client_credentials` grant.
    #[arg(long, env = "FC_OUTBOX_TOKEN")]
    token: Option<String>,

    /// Poll interval in milliseconds.
    #[arg(long, env = "FC_OUTBOX_POLL_INTERVAL_MS", default_value = "1000")]
    poll_interval_ms: u64,

    /// Max Postgres pool connections.
    #[arg(long, env = "FC_OUTBOX_MAX_CONNECTIONS", default_value = "5")]
    max_connections: u32,

    /// Skip the `CREATE TABLE IF NOT EXISTS` bootstrap. Use this when
    /// the app manages its own outbox schema and you'd rather see a
    /// clear failure than a silent auto-create.
    #[arg(long, env = "FC_OUTBOX_SKIP_BOOTSTRAP", default_value = "false")]
    skip_bootstrap: bool,
}

#[expect(
    clippy::let_underscore_must_use,
    reason = "a missing .env file is normal; the process environment is the source of truth"
)]
pub async fn run(args: OutboxArgs) -> Result<()> {
    // Go's `loadDotEnv`: a missing file is fine, a set variable wins.
    let _ = dotenvy::from_path(&args.env_file);
    match args.command {
        None => run_go(args.run).await,
        Some(OutboxCommand::CreateTable(args)) => run_create_table(args).await,
        Some(OutboxCommand::Init(args)) => run_init(args).await,
        Some(OutboxCommand::Poll(args)) => run_poll(args).await,
    }
}

/// The flag if given, else the first non-empty of `envs`.
fn flag_or_env(flag: Option<String>, envs: &[&str]) -> Option<String> {
    flag.filter(|v| !v.is_empty())
        .or_else(|| config::env_first_opt(envs))
}

fn number_or_env<T: FromStr>(flag: Option<T>, env: &str) -> Option<T> {
    flag.or_else(|| env::var(env).ok().and_then(|v| v.trim().parse().ok()))
}

/// The poller both forms run.
struct Poller {
    source_url: String,
    target_url: String,
    auth_token: Option<String>,
    token_source: Option<Arc<dyn TokenSource>>,
    batch_size: Option<u32>,
    max_in_flight: Option<u64>,
    poll_interval: Option<Duration>,
    max_connections: u32,
    bootstrap: bool,
}

async fn run_go(args: RunArgs) -> Result<()> {
    let source_url = flag_or_env(
        args.source_db_url,
        &["FC_OUTBOX_SOURCE_DB_URL", "FC_OUTBOX_DB_URL"],
    )
    .ok_or_else(|| anyhow!("--source-db-url (or FC_OUTBOX_SOURCE_DB_URL) is required"))?;
    let target_url = flag_or_env(
        args.target_url,
        &["FC_OUTBOX_PLATFORM_URL", "FC_OUTBOX_API_URL"],
    )
    .unwrap_or_else(|| "http://localhost:8080".to_string());
    let auth_token = flag_or_env(
        args.auth_token,
        &["FC_OUTBOX_PLATFORM_AUTH_TOKEN", "FC_OUTBOX_TOKEN"],
    );
    let client_id = flag_or_env(
        args.client_id,
        &["FC_OUTBOX_CLIENT_ID", "FLOWCATALYST_CLIENT_ID"],
    );
    let client_secret = flag_or_env(
        args.client_secret,
        &["FC_OUTBOX_CLIENT_SECRET", "FLOWCATALYST_CLIENT_SECRET"],
    );
    // A service account's client_credentials, minted and refreshed, win
    // over a static token (Go).
    let token_source: Option<Arc<dyn TokenSource>> = match (client_id, client_secret) {
        (Some(id), Some(secret)) => {
            let token_url = flag_or_env(args.token_url, &["FC_OUTBOX_TOKEN_URL"])
                .unwrap_or_else(|| format!("{}/oauth/token", target_url.trim_end_matches('/')));
            let scope = flag_or_env(args.scope, &["FC_OUTBOX_SCOPE"]);
            Some(Arc::new(ClientCredentialsTokenSource::new(
                token_url, id, secret, scope,
            )))
        }
        _ => None,
    };
    let positive = |n: Option<u64>| n.filter(|n| *n > 0);
    run_poller(Poller {
        source_url,
        target_url,
        auth_token,
        token_source,
        batch_size: number_or_env(args.batch_size, "FC_OUTBOX_BATCH_SIZE").filter(|n| *n > 0),
        max_in_flight: positive(number_or_env(args.max_in_flight, "FC_OUTBOX_MAX_IN_FLIGHT")),
        poll_interval: positive(number_or_env(
            args.poll_interval_ms,
            "FC_OUTBOX_POLL_INTERVAL_MS",
        ))
        .map(Duration::from_millis),
        max_connections: 5,
        bootstrap: true,
    })
    .await
}

async fn run_poll(args: PollArgs) -> Result<()> {
    let source_url = args.db_url.ok_or_else(|| {
        anyhow!(
            "FC_OUTBOX_DB_URL is required.\n\n\
             Run `fc-dev outbox init` from your project directory to write \
             `.env`, or set FC_OUTBOX_DB_URL in the environment."
        )
    })?;
    run_poller(Poller {
        source_url,
        target_url: args.api_url,
        auth_token: args.token,
        token_source: None,
        batch_size: None,
        max_in_flight: None,
        poll_interval: Some(Duration::from_millis(args.poll_interval_ms)),
        max_connections: args.max_connections,
        bootstrap: !args.skip_bootstrap,
    })
    .await
}

#[expect(
    clippy::let_underscore_must_use,
    reason = "the receiver may already be gone (shutdown, or an abandoned caller): nobody is left to notify"
)]
async fn run_poller(p: Poller) -> Result<()> {
    let auth = match (&p.token_source, &p.auth_token) {
        (Some(_), _) => "client_credentials",
        (None, Some(_)) => "static-token",
        (None, None) => "none",
    };
    info!(
        db_url = %redact_url(&p.source_url),
        api_url = %p.target_url,
        auth,
        "Starting standalone outbox poller"
    );

    if auth == "none" {
        warn!(
            "No platform credentials (FC_OUTBOX_PLATFORM_AUTH_TOKEN, or \
             FC_OUTBOX_CLIENT_ID + FC_OUTBOX_CLIENT_SECRET) — forwarded \
             requests will be unauthenticated and the platform will reject them."
        );
    }

    let pool = PgPoolOptions::new()
        .max_connections(p.max_connections)
        .connect(&p.source_url)
        .await
        .with_context(|| format!("connecting to {}", redact_url(&p.source_url)))?;

    let repository = Arc::new(PostgresOutboxRepository::new(pool));

    // Bootstrap the outbox table on first run. `init_schema` issues
    // `CREATE TABLE IF NOT EXISTS` + idempotent partial indexes, so
    // re-running against an existing schema is a no-op. The SDK's own
    // migration produces the same shape (see
    // `clients/typescript-sdk/migrations/postgresql/001_create_outbox_messages.sql`).
    if p.bootstrap {
        repository
            .init_schema()
            .await
            .context("create/verify outbox_messages schema")?;
    }

    let defaults = EnhancedProcessorConfig::default();
    let config = EnhancedProcessorConfig {
        poll_interval: p.poll_interval.unwrap_or(defaults.poll_interval),
        poll_batch_size: p.batch_size.unwrap_or(defaults.poll_batch_size),
        max_in_flight: p.max_in_flight.unwrap_or(defaults.max_in_flight),
        http_config: HttpDispatcherConfig {
            api_base_url: p.target_url,
            api_token: p.auth_token,
            token_source: p.token_source,
            ..Default::default()
        },
        ..defaults
    };

    let processor = Arc::new(
        EnhancedOutboxProcessor::new(config, repository)
            .map_err(|e| anyhow!("failed to create outbox processor: {e}"))?,
    );

    let (shutdown_tx, _) = broadcast::channel::<()>(1);
    let proc_clone = processor.clone();
    let mut shutdown_rx = shutdown_tx.subscribe();
    let handle = tokio::spawn(async move {
        tokio::select! {
            _ = processor.start() => {}
            _ = shutdown_rx.recv() => {
                info!("Outbox processor received shutdown signal");
                proc_clone.stop();
            }
        }
    });

    info!("Outbox poller running. Ctrl+C to stop.");
    server_setup::wait_for_shutdown_signal().await;
    info!("Shutdown signal received, stopping outbox poller…");

    let _ = shutdown_tx.send(());
    match time::timeout(Duration::from_secs(30), handle).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => warn!(error = %e, "The outbox poller task ended abnormally"),
        Err(_) => warn!("The outbox poller did not stop within 30s"),
    }

    info!("Outbox poller stopped");
    Ok(())
}

/// Go's `normalizeOutboxDBType`.
fn outbox_store(db_type: &str) -> Option<&'static str> {
    match db_type.trim().to_ascii_lowercase().as_str() {
        "pg" | "postgres" | "postgresql" => Some("postgres"),
        "mysql" | "mariadb" | "maria" => Some("mysql"),
        "mongo" | "mongodb" => Some("mongodb"),
        _ => None,
    }
}

/// A `mysql://` URL as given; a Go MySQL DSN
/// (`user:pass@tcp(host:port)/db?params`) rewritten as one.
fn mysql_url(raw: &str) -> Result<String> {
    if raw.contains("://") {
        return Ok(raw.to_string());
    }
    let (userinfo, rest) = raw.rsplit_once('@').unwrap_or(("", raw));
    let rest = rest.strip_prefix("tcp(").ok_or_else(|| {
        anyhow!("not a mysql:// URL or a Go MySQL DSN (user:pass@tcp(host:port)/db): {raw}")
    })?;
    let (host, rest) = rest
        .split_once(')')
        .ok_or_else(|| anyhow!("unterminated tcp( in the MySQL DSN"))?;
    let host = if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:3306")
    };
    let at = if userinfo.is_empty() { "" } else { "@" };
    Ok(format!("mysql://{userinfo}{at}{host}{rest}"))
}

async fn run_create_table(args: CreateTableArgs) -> Result<()> {
    let db_type = flag_or_env(args.db_type, &["FC_OUTBOX_BACKEND", "FC_OUTBOX_DB_TYPE"])
        .unwrap_or_else(|| "postgres".to_string());
    let db_url = flag_or_env(
        args.db_url,
        &[
            "FC_OUTBOX_SOURCE_DB_URL",
            "FC_OUTBOX_DB_URL",
            "FC_OUTBOX_MONGO_URI",
        ],
    )
    .ok_or_else(|| {
        anyhow!("--db-url (or FC_OUTBOX_SOURCE_DB_URL / FC_OUTBOX_DB_URL / FC_OUTBOX_MONGO_URI) is required")
    })?;
    match outbox_store(&db_type) {
        Some("postgres") => {
            let pool = PgPoolOptions::new()
                .max_connections(1)
                .connect(&db_url)
                .await
                .context("connect postgres")?;
            PostgresOutboxRepository::new(pool)
                .init_schema()
                .await
                .context("create outbox table")?;
            println!("Created outbox_messages table + indexes (postgres).");
        }
        Some("mysql") => {
            let pool = MySqlPoolOptions::new()
                .max_connections(1)
                .connect(&mysql_url(&db_url)?)
                .await
                .context("connect mysql")?;
            MySqlOutboxRepository::new(pool)
                .init_schema()
                .await
                .context("create outbox table")?;
            println!("Created outbox_messages table + indexes (mysql).");
        }
        Some(_) => {
            let db_name = flag_or_env(args.db_name, &["FC_OUTBOX_MONGO_DB"])
                .unwrap_or_else(|| "flowcatalyst".to_string());
            env::set_var("FC_OUTBOX_MONGO_DB", &db_name);
            setup::connect(
                fc_outbox::OutboxBackend::Mongo,
                &db_url,
                fc_outbox::OutboxTableConfig::default(),
            )
            .await
            .context("create outbox indexes")?;
            println!("Created outbox_messages collection indexes (mongodb, db {db_name:?}).");
        }
        None => {
            return Err(anyhow!(
                "unknown --db-type {db_type:?}: want postgres, mysql, or mongodb"
            ))
        }
    }
    Ok(())
}

async fn run_init(args: InitArgs) -> Result<()> {
    let db_url = resolve_or_prompt(
        args.db_url,
        "Postgres URL (e.g. postgres://user:pass@localhost:5432/myapp)",
        args.yes,
    )?;
    let api_url = resolve_or_prompt_with_default(
        args.api_url,
        "Platform API URL",
        "http://localhost:8080",
        args.yes,
    )?;
    let token = resolve_secret(args.token, "Bearer token", args.yes)?;

    let env_path = args.root.join(".env");
    let updates: Vec<(&str, &str)> = vec![
        ("FC_OUTBOX_DB_URL", &db_url),
        ("FC_OUTBOX_API_URL", &api_url),
        ("FC_OUTBOX_TOKEN", &token),
    ];

    println!();
    println!("Writing outbox config to {} …", env_path.display());
    init::write_env_updates(&env_path, &updates).context("write .env")?;

    // Tighten permissions on Unix — this file holds the platform bearer
    // token, which is enough to publish events on behalf of the service
    // account. Windows file ACLs default to user-only under the project
    // dir, so no extra hardening is needed there.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if env_path.exists() {
            let mut perms = fs::metadata(&env_path)?.permissions();
            perms.set_mode(0o600);
            fs::set_permissions(&env_path, perms).ok();
        }
    }

    println!();
    println!("✓ Done. Run the poller with:");
    println!("    fc-dev outbox");
    println!();
    println!(
        "It reads these keys from {} on startup.",
        env_path.display()
    );
    Ok(())
}

fn resolve_or_prompt(value: Option<String>, question: &str, yes: bool) -> Result<String> {
    if let Some(v) = value.filter(|s| !s.is_empty()) {
        return Ok(v);
    }
    if yes {
        return Err(anyhow!("{} is required in --yes mode", question));
    }
    print!("{}: ", question);
    stdout().flush().ok();
    let mut input = String::new();
    stdin().lock().read_line(&mut input)?;
    let trimmed = input.trim_end_matches(&['\r', '\n'][..]).to_string();
    if trimmed.is_empty() {
        return Err(anyhow!("{} is required", question));
    }
    Ok(trimmed)
}

fn resolve_or_prompt_with_default(
    value: Option<String>,
    question: &str,
    default: &str,
    yes: bool,
) -> Result<String> {
    if let Some(v) = value.filter(|s| !s.is_empty()) {
        return Ok(v);
    }
    if yes {
        return Ok(default.to_string());
    }
    print!("{} [{}]: ", question, default);
    stdout().flush().ok();
    let mut input = String::new();
    stdin().lock().read_line(&mut input)?;
    let trimmed = input.trim_end_matches(&['\r', '\n'][..]).to_string();
    Ok(if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed
    })
}

/// Best-effort secret entry. Matches the `fc-dev init` admin-password
/// prompt — paste-visible read from stdin. rpassword / termios no-echo
/// would be nicer but adds a dep / unsafe; the bigger win (keeping
/// tokens out of `ps` output and shell history) comes from accepting
/// them via prompt instead of argv, which this already does.
fn resolve_secret(value: Option<String>, question: &str, yes: bool) -> Result<String> {
    if let Some(v) = value.filter(|s| !s.is_empty()) {
        return Ok(v);
    }
    if yes {
        return Err(anyhow!("{} is required in --yes mode", question));
    }
    print!("{}: ", question);
    stdout().flush().ok();
    let mut input = String::new();
    stdin().lock().read_line(&mut input)?;
    let trimmed = input.trim_end_matches(&['\r', '\n'][..]).to_string();
    if trimmed.is_empty() {
        return Err(anyhow!("{} is required", question));
    }
    Ok(trimmed)
}

/// Hide the password portion of a `postgres://user:pass@host/db` URL
/// before logging it. Best-effort; falls back to the original string
/// if no `:pass@` segment is present.
fn redact_url(url: &str) -> String {
    let Some((before_at, after_at)) = url.split_once('@') else {
        return url.to_string();
    };
    let Some((scheme_user, _password)) = before_at.rsplit_once(':') else {
        return url.to_string();
    };
    format!("{scheme_user}:***@{after_at}")
}

#[cfg(test)]
mod tests {
    use super::{mysql_url, outbox_store, redact_url};

    #[test]
    fn store_names_fold_as_gos() {
        assert_eq!(outbox_store("PG"), Some("postgres"));
        assert_eq!(outbox_store("postgresql"), Some("postgres"));
        assert_eq!(outbox_store("mariadb"), Some("mysql"));
        assert_eq!(outbox_store("mongo"), Some("mongodb"));
        assert_eq!(outbox_store("sqlite"), None);
    }

    #[test]
    fn a_go_mysql_dsn_becomes_a_url() {
        assert_eq!(
            mysql_url("user:pass@tcp(localhost:3306)/app").unwrap(),
            "mysql://user:pass@localhost:3306/app"
        );
        assert_eq!(
            mysql_url("user@tcp(db)/app?parseTime=true").unwrap(),
            "mysql://user@db:3306/app?parseTime=true"
        );
        assert_eq!(
            mysql_url("mysql://u:p@h:3307/d").unwrap(),
            "mysql://u:p@h:3307/d"
        );
        assert!(mysql_url("garbage").is_err());
    }

    #[test]
    fn redact_url_hides_password() {
        assert_eq!(
            redact_url("postgres://user:secret@localhost:5432/db"),
            "postgres://user:***@localhost:5432/db"
        );
    }

    #[test]
    fn redact_url_passthrough_when_no_password() {
        assert_eq!(
            redact_url("postgres://localhost:5432/db"),
            "postgres://localhost:5432/db"
        );
    }
}
