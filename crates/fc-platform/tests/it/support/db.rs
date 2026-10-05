//! Where an integration test's PostgreSQL comes from.
//!
//! By default every test starts a throwaway Docker container (testcontainers).
//! Two environment variables replace Docker, for machines without it:
//!
//! * `FC_TEST_PG_BIN=<dir>`: a PostgreSQL `bin` directory (it must hold
//!   `initdb` and `postgres`). The harness initialises a private cluster in a
//!   temporary directory and starts it on a free loopback port. Any
//!   PostgreSQL 15+ distribution works, e.g. the embedded-postgres archive
//!   the Go repo's tests download to `~/.embedded-postgres-go/`
//!   (`tar -xJf embedded-postgres-binaries-*.txz -C <dir>`; its `bin/` has no
//!   `psql`, which the tests do not need). One cluster is started per test
//!   process, on first use, and each test gets its own database on it
//!   (a cluster per test would exhaust the machine's 32 SysV shared-memory
//!   segments on macOS); it is stopped when the process ends.
//! * `FC_TEST_DATABASE_URL=<url>`: an already running server (the URL must
//!   be able to `CREATE DATABASE`). Each test gets its own database on it,
//!   dropped at the end of the test.
//!
//! `FC_TEST_PG_BIN` wins when both are set. A test that is marked
//! `#[ignore = "requires Docker"]` is still ignored by the harness; run it
//! with `--include-ignored` (or `--ignored`) as usual.

use std::env;
use std::fs::{self, File};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use sqlx::{Connection, PgConnection};
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;
use tokio::runtime::Builder;
use tokio::time::sleep;

/// Keeps a test's database alive; dropping it tears the database down.
pub enum TestDb {
    Container(Box<ContainerAsync<Postgres>>),
    /// A database on a server shared by the tests of this process (the
    /// cluster `FC_TEST_PG_BIN` starts, or the one `FC_TEST_DATABASE_URL`
    /// names): dropped with `DROP DATABASE`.
    Database {
        admin_url: String,
        name: String,
    },
}

impl Drop for TestDb {
    fn drop(&mut self) {
        if let TestDb::Database { admin_url, name } = self {
            let (admin_url, name) = (admin_url.clone(), name.clone());
            // On a thread of its own: a test's runtime may already be
            // shutting down.
            let _ = thread::spawn(move || {
                let Ok(rt) = Builder::new_current_thread().enable_all().build() else {
                    return;
                };
                rt.block_on(async {
                    if let Ok(mut conn) = PgConnection::connect(&admin_url).await {
                        let _ = sqlx::query(&format!(
                            "DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)"
                        ))
                        .execute(&mut conn)
                        .await;
                    }
                });
            })
            .join();
        }
    }
}

/// Whether the tests run on a local PostgreSQL instead of Docker.
pub fn docker_free() -> bool {
    env::var_os("FC_TEST_PG_BIN").is_some() || env::var_os("FC_TEST_DATABASE_URL").is_some()
}

/// A fresh, empty database named `name` (migrations are the caller's):
/// returns what keeps it alive and its connection URL.
pub async fn start_db(name: &str) -> (TestDb, String) {
    if let Some(bin) = env::var_os("FC_TEST_PG_BIN") {
        return start_on_server(shared_cluster(&PathBuf::from(bin)).to_string(), name).await;
    }
    if let Ok(admin_url) = env::var("FC_TEST_DATABASE_URL") {
        return start_on_server(admin_url, name).await;
    }
    let container = Postgres::default()
        .with_db_name(name)
        .with_user("test")
        .with_password("test")
        .start()
        .await
        .expect("failed to start Postgres container (set FC_TEST_PG_BIN to run without Docker)");
    let host = container.get_host().await.expect("host");
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    let url = format!("postgresql://test:test@{host}:{port}/{name}");
    (TestDb::Container(Box::new(container)), url)
}

async fn start_on_server(admin_url: String, name: &str) -> (TestDb, String) {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let unique = format!(
        "fc_test_{}_{}_{}",
        process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed),
        name.replace(|c: char| !c.is_ascii_alphanumeric(), "_")
    );
    let started = Instant::now();
    let mut conn = loop {
        match PgConnection::connect(&admin_url).await {
            Ok(c) => break c,
            Err(e) if started.elapsed() > Duration::from_secs(60) => {
                panic!("connect to the test server {admin_url}: {e}")
            }
            Err(_) => sleep(Duration::from_millis(50)).await,
        }
    };
    sqlx::query(&format!("CREATE DATABASE \"{unique}\""))
        .execute(&mut conn)
        .await
        .expect("create the test database");
    // The same server and credentials, the new database in place of the path.
    let (base, query) = admin_url
        .split_once('?')
        .map_or((admin_url.as_str(), None), |(b, q)| (b, Some(q)));
    let (server, _) = base
        .rsplit_once('/')
        .filter(|(s, _)| s.contains("://"))
        .expect("FC_TEST_DATABASE_URL is postgresql://user:pass@host:port/db");
    let mut url = format!("{server}/{unique}");
    if let Some(q) = query {
        url.push('?');
        url.push_str(q);
    }
    (
        TestDb::Database {
            admin_url,
            name: unique,
        },
        url,
    )
}

/// The admin URL of the one cluster this process runs from `bin`, started
/// on first use. One cluster for all the process's tests, a database each:
/// a cluster per test would use a SysV shared-memory segment each, and macOS
/// allows only 32 of them for the whole machine.
///
/// The cluster is stopped (a fast shutdown, which releases its segment) and
/// its directory deleted by a small watcher shell that outlives the test
/// process by at most a second, however the process ends (even SIGKILL).
fn shared_cluster(bin: &Path) -> &'static str {
    static CLUSTER: OnceLock<String> = OnceLock::new();
    CLUSTER.get_or_init(|| start_cluster(bin))
}

// The postgres child is not waited on here: the watcher below stops it, and
// when this process ends init reaps it.
#[allow(clippy::zombie_processes)]
fn start_cluster(bin: &Path) -> String {
    // Kept (never dropped): the watcher deletes it.
    let dir = tempfile::Builder::new()
        .prefix("fc-test-pg-")
        .tempdir()
        .expect("temp dir")
        .keep();
    let data = dir.join("data");
    let init = Command::new(bin.join("initdb"))
        .arg("-D")
        .arg(&data)
        .args([
            "-U",
            "test",
            "--auth=trust",
            "-E",
            "UTF8",
            "--no-locale",
            "--no-sync",
        ])
        .output()
        .unwrap_or_else(|e| panic!("run {}/initdb (FC_TEST_PG_BIN): {e}", bin.display()));
    assert!(
        init.status.success(),
        "initdb failed: {}{}",
        String::from_utf8_lossy(&init.stdout),
        String::from_utf8_lossy(&init.stderr)
    );

    let port = free_port();
    let log = File::create(dir.join("postgres.log")).expect("log file");
    let child = Command::new(bin.join("postgres"))
        .arg("-D")
        .arg(&data)
        .args(["-p", &port.to_string()])
        .arg("-k")
        .arg(&dir)
        .args([
            "-c",
            "listen_addresses=127.0.0.1",
            "-c",
            "fsync=off",
            "-c",
            "synchronous_commit=off",
            "-c",
            "full_page_writes=off",
            "-c",
            "max_connections=400",
            "-c",
            "shared_buffers=128MB",
        ])
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()
        .unwrap_or_else(|e| panic!("run {}/postgres: {e}", bin.display()));

    // The watcher: when this process is gone, stop the cluster and delete it.
    Command::new("sh")
        .arg("-c")
        .arg(
            "while kill -0 \"$1\" 2>/dev/null; do sleep 1; done; \
             kill -INT \"$2\" 2>/dev/null; \
             while kill -0 \"$2\" 2>/dev/null; do sleep 0.2; done; \
             rm -rf \"$3\"",
        )
        .arg("fc-test-pg-watcher")
        .arg(process::id().to_string())
        .arg(child.id().to_string())
        .arg(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the cluster's watcher");

    let started = Instant::now();
    while TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_err() {
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "local postgres did not start:\n{}",
            fs::read_to_string(dir.join("postgres.log")).unwrap_or_default()
        );
        thread::sleep(Duration::from_millis(50));
    }
    format!("postgresql://test:test@127.0.0.1:{port}/postgres")
}

/// A loopback port nothing is listening on right now.
fn free_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("a free port")
}
