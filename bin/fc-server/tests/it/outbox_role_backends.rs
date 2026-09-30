//! fc-server's outbox role (`FC_OUTBOX_ENABLED`) reads a MySQL outbox:
//! consumer applications keep their outbox tables in MySQL as well as
//! Postgres. The role runs `fc_outbox::setup::connect`, as
//! fc-outbox-processor does; this build must carry the MySQL backend. It
//! refuses an SQLite outbox (owner decision #52).

use std::env;
use std::fs;
use std::io::Read;
use std::process;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use fc_outbox::{setup, OutboxBackend, OutboxTableConfig};

#[tokio::test]
async fn the_outbox_role_carries_the_mysql_backend() {
    // The MySQL driver parses the URL and refuses its bad option at once,
    // rather than the backend being "not built into this binary".
    let err = match setup::connect(
        OutboxBackend::Mysql,
        "mysql://root@127.0.0.1:1/app?ssl-mode=bogus",
        OutboxTableConfig::default(),
    )
    .await
    {
        Ok(_) => panic!("the URL is invalid"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("ssl_mode"), "{err}");
}

/// Owner decision #52: fc-server carries no SQLite driver, so an SQLite
/// outbox is refused at startup, before anything connects, with an error
/// naming fc-outbox-processor (which reads it). Not a panic, and not a
/// silently disabled outbox role.
#[test]
fn the_outbox_role_refuses_an_sqlite_outbox() {
    let dir = env::temp_dir().join(format!("fc-server-sqlite-outbox-{}", process::id()));
    fs::create_dir_all(&dir).unwrap();
    let db_file = dir.join("outbox.db");
    let mut child = Command::new(env!("CARGO_BIN_EXE_fc-server"))
        .env_clear()
        .env("PATH", env::var("PATH").unwrap_or_default())
        .env("HOME", &dir)
        .env("FC_PLATFORM_ENABLED", "false")
        .env("FC_OUTBOX_ENABLED", "true")
        .env("FC_OUTBOX_BACKEND", "sqlite")
        .env(
            "FC_OUTBOX_DB_URL",
            format!("sqlite://{}?mode=rwc", db_file.display()),
        )
        // Nothing listens here: reaching the database would be a different error.
        .env("FC_DATABASE_URL", "postgresql://127.0.0.1:1/flowcatalyst")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("fc-server did not refuse the sqlite outbox within 30s");
        }
        thread::sleep(Duration::from_millis(50));
    };
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();

    assert!(!status.success(), "{stderr}");
    assert!(
        stderr.contains(
            "The sqlite outbox backend is not supported by fc-server; run fc-outbox-processor instead"
        ),
        "{stderr}"
    );
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert!(!db_file.exists(), "nothing opened the SQLite outbox");
    let _ = fs::remove_dir_all(&dir);
}
