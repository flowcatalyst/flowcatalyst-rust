//! The embedded PostgreSQL 18 cluster fc-dev shares with Go's and Java's
//! `fcdev`.
//!
//! Same cluster, port, credentials and database as Go (`cmd/fcdev/start.go`,
//! `embedded.go`) and Java (`EmbeddedPg`):
//!
//! - cluster in `<path>/data`, `<path>` = `--embedded-db-path` /
//!   `FC_EMBEDDED_DB_PATH`, default `<userDataDir>/flowcatalyst/embedded-pg`
//!   (`~/Library/Application Support/flowcatalyst/embedded-pg` on macOS);
//! - port 15432 (`--embedded-db-port` / `FC_EMBEDDED_DB_PORT`, 0 = any free);
//! - superuser `postgres` / `postgres`, database `flowcatalyst`:
//!   `postgresql://postgres:postgres@localhost:15432/flowcatalyst?sslmode=disable`;
//! - PostgreSQL major pinned to 18; a cluster of another major is refused
//!   with Go's `assertEmbeddedVersionCompatible` message.
//!
//! The server binaries are the theseus PostgreSQL build bundled into the
//! fc-dev executable (`postgresql_embedded`'s `bundled` feature), extracted
//! once into `<userCacheDir>/flowcatalyst/embedded-pg/theseus/<version>`,
//! beside Go's (`bin/`) and Java's (`PG-<md5>/`) trees. A fresh machine with
//! no Go or Java install gets a working cluster from fc-dev alone: `initdb`
//! runs with Go's arguments (`-A password -U postgres`, password `postgres`,
//! UTF8) when `<path>/data` has no cluster yet.
//!
//! PostGIS: see [`crate::pg_extensions`]. fc-dev mirrors the PostGIS files
//! from Go's tree (or another source) into its own before starting, so a
//! cluster with `CREATE EXTENSION postgis` keeps working under it.
//!
//! One instance at a time: see [`crate::instance_guard`]. `postgresql_embedded`
//! only extracts binaries and runs `initdb`; fc-dev runs `pg_ctl` itself, so
//! nothing ever stops a server it did not start (the crate's `Drop` would run
//! `pg_ctl stop` on any cluster with a `postmaster.pid`, including one Go or
//! Java is serving).

use anyhow::{bail, Context, Result};
use postgresql_embedded::{PostgreSQL, Settings};
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tracing::{error, info, warn};

use crate::dev_paths;
use crate::instance_guard::{self, ClusterLock};
use crate::pg_extensions;

/// The PostgreSQL major the shared cluster runs (Go `embeddedPGVersion`,
/// Java's zonky binaries). The bundled archive is pinned to it by
/// `POSTGRESQL_VERSION` in `.cargo/config.toml`.
pub const PG_MAJOR: u64 = 18;

pub const USER: &str = "postgres";
pub const PASSWORD: &str = "postgres";
pub const DATABASE: &str = "flowcatalyst";

const START_TIMEOUT: Duration = Duration::from_secs(60);

/// The embedded-database flags `start`, `init` and `fresh` share (Go's and
/// Java's names).
#[derive(clap::Args, Debug, Clone)]
pub struct EmbeddedDbArgs {
    /// Embedded Postgres directory; the cluster is `<DIR>/data`. Shared with
    /// Go's and Java's fcdev. [default: <userDataDir>/flowcatalyst/embedded-pg,
    /// on macOS ~/Library/Application Support/flowcatalyst/embedded-pg]
    #[arg(long, env = "FC_EMBEDDED_DB_PATH", value_name = "DIR")]
    pub embedded_db_path: Option<PathBuf>,

    /// Embedded Postgres port (0 picks a free one).
    #[arg(long, env = "FC_EMBEDDED_DB_PORT", default_value_t = dev_paths::DEFAULT_EMBEDDED_DB_PORT)]
    pub embedded_db_port: u16,

    /// A PostgreSQL 18 installation to copy PostGIS from when fc-dev's own
    /// tree lacks it (tried before Go's fcdev tree, Java's, Homebrew and
    /// the PGDG packages).
    #[arg(long, env = "FC_EMBEDDED_DB_EXTENSIONS_FROM", value_name = "DIR")]
    pub embedded_db_extensions_from: Option<PathBuf>,
}

impl EmbeddedDbArgs {
    pub fn path(&self) -> PathBuf {
        dev_paths::embedded_path(self.embedded_db_path.as_ref())
    }
}

/// Wiping the cluster: `--embedded-db-reset` plus, for the shared default
/// cluster, the confirmation.
#[derive(Debug, Clone, Copy, Default)]
pub struct Reset {
    pub requested: bool,
    pub confirmed: bool,
}

/// Whether `start` must be the only server on the cluster, or may use one
/// that is already running (`init`, `fresh`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// `fc-dev start`: refuse when anything already serves the cluster.
    Exclusive,
    /// `fc-dev init` / `fresh`: connect to the running server, or start one
    /// for the duration of the command.
    AttachOrStart,
}

/// The cluster fc-dev is connected to. Stops the server on drop when (and
/// only when) fc-dev started it.
pub struct EmbeddedDb {
    pub url: String,
    cluster_dir: PathBuf,
    bin_dir: PathBuf,
    started_here: bool,
    _lock: Option<ClusterLock>,
}

impl Drop for EmbeddedDb {
    fn drop(&mut self) {
        if self.started_here {
            self.started_here = false;
            pg_ctl_stop(&self.bin_dir, &self.cluster_dir);
        }
    }
}

/// The libpq URL for the shared cluster on `port`.
pub fn url(port: u16) -> String {
    format!("postgresql://{USER}:{PASSWORD}@localhost:{port}/{DATABASE}?sslmode=disable")
}

/// The version of the PostgreSQL archive bundled into this binary.
fn bundled_version(settings: &Settings) -> Result<(u64, String)> {
    let c = settings
        .version
        .comparators
        .first()
        .context("bundled PostgreSQL version is unknown")?;
    let full = format!(
        "{}.{}.{}",
        c.major,
        c.minor.unwrap_or(0),
        c.patch.unwrap_or(0)
    );
    Ok((c.major, full))
}

/// Go `embeddedDataMajor`: `<path>/data/PG_VERSION`, `None` without a cluster.
pub fn data_major(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path.join("data").join("PG_VERSION")) {
        Ok(s) => Ok(Some(s.trim().to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).context("read embedded PG_VERSION"),
    }
}

/// Go `assertEmbeddedVersionCompatible`.
pub fn assert_version_compatible(path: &Path) -> Result<()> {
    match data_major(path)? {
        None => Ok(()),
        Some(have) if have == PG_MAJOR.to_string() => Ok(()),
        Some(have) => bail!(
            "embedded Postgres data dir {} is PG{have} but this fc-dev embeds PG{PG_MAJOR}; a major \
             upgrade is not in-place. Upgrade it with Go's or Java's 'fcdev db upgrade' (backs up the \
             old cluster, re-initialises PG{PG_MAJOR}, re-runs migrations + seed) or wipe it with \
             'fc-dev --embedded-db-reset'",
            path.join("data").display()
        ),
    }
}

/// `Settings::default()` makes two temp directories it never removes;
/// clear them (they are empty).
fn default_settings() -> Settings {
    let s = Settings::default();
    let _ = std::fs::remove_dir(&s.data_dir);
    if let Some(parent) = s.password_file.parent() {
        let _ = std::fs::remove_dir(parent);
    }
    s
}

/// Start (or attach to) the shared cluster.
pub async fn start(
    args: &EmbeddedDbArgs,
    reset: Reset,
    mode: Mode,
    pid_file: &Path,
) -> Result<EmbeddedDb> {
    let path = args.path();
    let cluster_dir = path.join("data");
    let pg_cache = dev_paths::embedded_pg_cache_dir();

    let base = default_settings();
    let (bundled_major, bundled_full) = bundled_version(&base)?;
    if bundled_major != PG_MAJOR {
        bail!(
            "this fc-dev bundles PostgreSQL {bundled_full}, but the shared embedded cluster is \
             PG{PG_MAJOR}; rebuild with POSTGRESQL_VERSION={PG_MAJOR} (see .cargo/config.toml)"
        );
    }
    let installation_dir = pg_cache.join("theseus").join(&bundled_full);
    let bin_dir = installation_dir.join("bin");

    // ── Is something already serving the cluster? ──────────────────────
    if let Some(pm) = instance_guard::running_postmaster(&cluster_dir) {
        let owner = instance_guard::describe_cluster_owner(pid_file, &pm);
        match mode {
            Mode::Exclusive => bail!(
                "the embedded cluster {} is already being served ({owner}). Rust, Go and Java \
                 fcdev share it and must not run at the same time: stop that one first \
                 (`fc-dev stop`, or `fcdev stop` with the Go/Java binary).",
                cluster_dir.display()
            ),
            Mode::AttachOrStart => {
                if reset.requested {
                    bail!("cannot reset the embedded cluster while it is running ({owner})");
                }
                let port = pm.port.unwrap_or(args.embedded_db_port);
                info!(
                    path = %cluster_dir.display(),
                    port,
                    "Using the embedded Postgres that is already running ({owner})"
                );
                return Ok(EmbeddedDb {
                    url: url(port),
                    cluster_dir,
                    bin_dir,
                    started_here: false,
                    _lock: None,
                });
            }
        }
    }

    // ── Reset ──────────────────────────────────────────────────────────
    if reset.requested {
        reset_cluster(&path, &dev_paths::default_embedded_path(), reset)?;
    }

    assert_version_compatible(&path)?;

    let legacy = dev_paths::legacy_rust_cluster();
    if legacy.join("PG_VERSION").exists() {
        info!(
            legacy = %legacy.display(),
            "fc-dev now shares the Go/Java embedded cluster; its previous Rust-only cluster is no \
             longer used (delete that directory once you no longer need it)"
        );
    }

    // ── Binaries (+ initdb on a fresh machine) ─────────────────────────
    std::fs::create_dir_all(&path)
        .with_context(|| format!("create embedded Postgres dir {}", path.display()))?;
    std::fs::create_dir_all(&pg_cache).with_context(|| format!("create {}", pg_cache.display()))?;
    let fresh_cluster = !cluster_dir.join("postgresql.conf").exists();
    let password_file =
        std::env::temp_dir().join(format!("fc-dev-pg-{}.pwfile", std::process::id()));
    let settings = Settings {
        installation_dir: installation_dir.clone(),
        data_dir: cluster_dir.clone(),
        password_file: password_file.clone(),
        username: USER.to_string(),
        password: PASSWORD.to_string(),
        temporary: false,
        timeout: Some(Duration::from_secs(300)),
        ..base
    };
    // The crate's `Drop` stops any cluster that has a `postmaster.pid`;
    // this value must never run it.
    let mut pg = ManuallyDrop::new(PostgreSQL::new(settings));
    if fresh_cluster {
        info!(path = %cluster_dir.display(), "Initialising a new embedded PostgreSQL {PG_MAJOR} cluster");
    }
    let setup = pg.setup().await;
    let _ = std::fs::remove_file(&password_file);
    setup.context("embedded Postgres setup (extract binaries / initdb) failed")?;

    // ── PostGIS into our tree, before the server reads it ──────────────
    provision_postgis(
        &installation_dir,
        args.embedded_db_extensions_from.as_deref(),
        &pg_cache,
    );

    // ── Start ──────────────────────────────────────────────────────────
    let lock = instance_guard::lock_cluster(&cluster_dir)?;
    let port = if args.embedded_db_port == 0 {
        free_port()?
    } else {
        args.embedded_db_port
    };
    let log_file = pg_cache.join("theseus").join("postgres.log");
    info!(
        path = %cluster_dir.display(),
        port,
        binaries = %installation_dir.display(),
        "Starting embedded PostgreSQL {bundled_full}"
    );
    pg_ctl_start(&bin_dir, &cluster_dir, port, &log_file).await?;
    let db = EmbeddedDb {
        url: url(port),
        cluster_dir,
        bin_dir,
        started_here: true,
        _lock: Some(lock),
    };

    ensure_database(port).await?;
    verify_extensions(port, &installation_dir).await;
    info!(url = %db.url, "Embedded Postgres ready");
    Ok(db)
}

/// Stop the server if fc-dev started it.
pub async fn stop(db: &mut EmbeddedDb) {
    if !db.started_here {
        return;
    }
    db.started_here = false;
    info!("Stopping embedded Postgres");
    let bin = db.bin_dir.clone();
    let cluster = db.cluster_dir.clone();
    let _ = tokio::task::spawn_blocking(move || pg_ctl_stop(&bin, &cluster)).await;
}

/// Go's `--embedded-db-reset`: delete the whole `<path>` (DEV-3). The shared
/// default cluster holds the developer's data for all three binaries, so
/// wiping it also needs the confirmation.
fn reset_cluster(path: &Path, shared_default: &Path, reset: Reset) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if same_path(path, shared_default) && !reset.confirmed {
        bail!(
            "--embedded-db-reset would delete {} — the embedded cluster Go's and Java's fcdev \
             share with fc-dev, holding every database in it. If that is what you want, add \
             --confirm-shared-db-reset (FC_CONFIRM_SHARED_DB_RESET=true).",
            path.display()
        );
    }
    warn!(path = %path.display(), "Wiping the embedded Postgres directory");
    std::fs::remove_dir_all(path)
        .with_context(|| format!("remove embedded Postgres dir {}", path.display()))
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

fn free_port() -> Result<u16> {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0)).context("pick a free port")?;
    Ok(l.local_addr()?.port())
}

fn pg_command(bin_dir: &Path, program: &str) -> Command {
    let mut cmd = Command::new(bin_dir.join(program));
    // A developer's libpq environment must not redirect pg_ctl.
    for var in [
        "PGDATA",
        "PGPORT",
        "PGHOST",
        "PGDATABASE",
        "PGUSER",
        "PGPASSWORD",
    ] {
        cmd.env_remove(var);
    }
    cmd
}

async fn pg_ctl_start(bin_dir: &Path, cluster_dir: &Path, port: u16, log: &Path) -> Result<()> {
    let mut cmd = pg_command(bin_dir, "pg_ctl");
    cmd.arg("start")
        .arg("-w")
        .arg("-t")
        .arg(START_TIMEOUT.as_secs().to_string())
        .arg("-D")
        .arg(cluster_dir)
        .arg("-l")
        .arg(log)
        .arg("-o")
        .arg(format!("-F -p {port}"));
    let out = tokio::task::spawn_blocking(move || cmd.output())
        .await
        .context("pg_ctl start")?
        .context("run pg_ctl start")?;
    if !out.status.success() {
        let tail = std::fs::read_to_string(log)
            .map(|s| {
                let lines: Vec<&str> = s.lines().collect();
                lines[lines.len().saturating_sub(15)..].join("\n")
            })
            .unwrap_or_default();
        bail!(
            "embedded Postgres failed to start on port {port}: {}{}\n--- {} (last lines) ---\n{tail}",
            String::from_utf8_lossy(&out.stdout).trim(),
            String::from_utf8_lossy(&out.stderr).trim(),
            log.display()
        );
    }
    Ok(())
}

fn pg_ctl_stop(bin_dir: &Path, cluster_dir: &Path) {
    let status = pg_command(bin_dir, "pg_ctl")
        .args(["stop", "-m", "fast", "-w", "-t", "60", "-D"])
        .arg(cluster_dir)
        .output();
    match status {
        Ok(out) if out.status.success() => {}
        Ok(out) => warn!(
            stderr = %String::from_utf8_lossy(&out.stderr).trim(),
            "Failed to stop embedded Postgres cleanly"
        ),
        Err(e) => warn!(error = %e, "Failed to run pg_ctl stop"),
    }
}

async fn connect(port: u16, database: &str) -> Result<sqlx::PgPool> {
    let url = format!("postgresql://{USER}:{PASSWORD}@localhost:{port}/{database}?sslmode=disable");
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&url)
        .await
        .with_context(|| {
            format!(
                "connect to the embedded Postgres as {USER}/{PASSWORD} (database {database}); \
                 the shared cluster expects Go's credentials"
            )
        })
}

/// Create the `flowcatalyst` database if the cluster doesn't have it.
async fn ensure_database(port: u16) -> Result<()> {
    let pool = connect(port, "postgres").await?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_database WHERE datname = $1)")
            .bind(DATABASE)
            .fetch_one(&pool)
            .await?;
    if !exists {
        sqlx::query(&format!("CREATE DATABASE \"{DATABASE}\""))
            .execute(&pool)
            .await
            .context("create the flowcatalyst database")?;
        info!(database = DATABASE, "Created embedded database");
    }
    pool.close().await;
    Ok(())
}

/// Copy PostGIS into fc-dev's tree when it lacks it and a donor has it.
/// Non-fatal: an optional extension must never keep the database down.
fn provision_postgis(installation_dir: &Path, configured: Option<&Path>, pg_cache: &Path) {
    let Some(target) = pg_extensions::locate(installation_dir, "plpgsql") else {
        warn!(
            path = %installation_dir.display(),
            "Could not locate the module/extension directories of fc-dev's Postgres tree; PostGIS not provisioned"
        );
        return;
    };
    if pg_extensions::has_postgis(&target) {
        return;
    }
    let candidates = pg_extensions::donor_candidates(configured, pg_cache, &PG_MAJOR.to_string());
    let Some(donor) = pg_extensions::first_usable(&candidates) else {
        return;
    };
    match pg_extensions::mirror(donor, &target) {
        Ok(copied) if !copied.is_empty() => info!(
            count = copied.len(),
            from = %donor.extensions.display(),
            into = %target.extensions.display(),
            "Provisioned PostGIS into fc-dev's Postgres tree"
        ),
        Ok(_) => {}
        Err(e) => warn!(
            error = %e,
            from = %donor.extensions.display(),
            "Copying PostGIS into fc-dev's Postgres tree failed"
        ),
    }
}

/// Log every extension installed in any database of the cluster that this
/// tree cannot serve. Queries touching those objects will fail; the
/// platform's own tables don't use any, so this is an error log, not a
/// refusal to start.
async fn verify_extensions(port: u16, installation_dir: &Path) {
    let Some(tree) = pg_extensions::locate(installation_dir, "plpgsql") else {
        return;
    };
    let databases: Vec<String> = match connect(port, "postgres").await {
        Ok(pool) => {
            let r = sqlx::query_scalar(
                "SELECT datname FROM pg_database WHERE datallowconn AND NOT datistemplate ORDER BY 1",
            )
            .fetch_all(&pool)
            .await;
            pool.close().await;
            r.unwrap_or_default()
        }
        Err(_) => return,
    };
    for db in databases {
        let Ok(pool) = connect(port, &db).await else {
            continue;
        };
        let installed: Vec<String> = sqlx::query_scalar("SELECT extname::text FROM pg_extension")
            .fetch_all(&pool)
            .await
            .unwrap_or_default();
        pool.close().await;
        let missing = pg_extensions::missing_control_files(&installed, &tree.extensions);
        if !missing.is_empty() {
            error!(
                database = %db,
                extensions = ?missing,
                tree = %tree.extensions.display(),
                "Database uses extension(s) fc-dev's Postgres tree does not provide, and no source \
                 tree had them (Go's fcdev tree, Java's, --embedded-db-extensions-from, Homebrew, \
                 PGDG). Queries touching those objects will fail. Install PostGIS for PostgreSQL \
                 {PG_MAJOR} (macOS: `brew install postgis`) or point --embedded-db-extensions-from \
                 at a PG {PG_MAJOR} tree that has it, then restart fc-dev"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_postgres_is_the_shared_clusters_major() {
        let (major, full) = bundled_version(&default_settings()).unwrap();
        assert_eq!(
            major, PG_MAJOR,
            "fc-dev bundles PostgreSQL {full}; the cluster shared with Go/Java is PG{PG_MAJOR}"
        );
    }

    #[test]
    fn url_matches_go_and_java() {
        assert_eq!(
            url(15432),
            "postgresql://postgres:postgres@localhost:15432/flowcatalyst?sslmode=disable"
        );
    }

    #[test]
    fn another_major_is_refused_with_gos_message() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            assert_version_compatible(dir.path()).is_ok(),
            "no cluster yet"
        );
        std::fs::create_dir_all(dir.path().join("data")).unwrap();
        std::fs::write(dir.path().join("data/PG_VERSION"), "18\n").unwrap();
        assert!(assert_version_compatible(dir.path()).is_ok());
        std::fs::write(dir.path().join("data/PG_VERSION"), "17\n").unwrap();
        let err = assert_version_compatible(dir.path())
            .unwrap_err()
            .to_string();
        assert!(err.contains("is PG17 but this fc-dev embeds PG18"), "{err}");
        assert!(err.contains("not in-place"), "{err}");
    }

    #[test]
    fn resetting_the_shared_default_cluster_needs_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let shared = dir.path().join("flowcatalyst").join("embedded-pg");
        std::fs::create_dir_all(shared.join("data")).unwrap();
        std::fs::write(shared.join("data/PG_VERSION"), "18\n").unwrap();
        let unconfirmed = Reset {
            requested: true,
            confirmed: false,
        };
        let err = reset_cluster(&shared, &shared, unconfirmed)
            .unwrap_err()
            .to_string();
        assert!(err.contains("--confirm-shared-db-reset"), "{err}");
        assert!(shared.join("data/PG_VERSION").exists(), "nothing deleted");

        let confirmed = Reset {
            requested: true,
            confirmed: true,
        };
        reset_cluster(&shared, &shared, confirmed).unwrap();
        assert!(!shared.exists(), "Go deletes the whole <path>");
    }

    #[test]
    fn another_path_resets_without_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let mine = dir.path().join("mine");
        std::fs::create_dir_all(mine.join("data")).unwrap();
        let reset = Reset {
            requested: true,
            confirmed: false,
        };
        reset_cluster(&mine, &dir.path().join("shared"), reset).unwrap();
        assert!(!mine.exists());
        // Nothing there: nothing to do.
        reset_cluster(&mine, &dir.path().join("shared"), reset).unwrap();
    }
}
