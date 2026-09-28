//! The `fc-outbox-processor` binary reads a MySQL outbox: consumer
//! applications keep their outbox tables in MySQL as well as Postgres.
//!
//! The first test needs nothing: it only proves the binary's build carries
//! the MySQL backend (a backend left out is refused by name, before any
//! connection). The second runs the built binary with
//! `FC_OUTBOX_BACKEND=mysql` against a MySQL 8 container and a fake platform,
//! and waits for a row written in the SDK's table shape to be delivered
//! (`cargo test -p fc-outbox-processor -- --ignored` with Docker running).

use std::net::TcpListener as StdListener;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::routing::post;
use axum::{Json, Router};
use fc_outbox::{setup, OutboxBackend, OutboxTableConfig};
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::mysql::Mysql;

#[tokio::test]
async fn this_build_carries_the_mysql_backend() {
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

/// Kills the child process on drop, so a failed assertion leaves nothing
/// running.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    StdListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test]
#[ignore = "needs Docker"]
async fn the_binary_delivers_a_mysql_outbox_row() {
    let container = Mysql::default().start().await.unwrap();
    let db_port = container.get_host_port_ipv4(3306).await.unwrap();
    let url = format!("mysql://root@127.0.0.1:{db_port}/test");

    // The platform's event batch API, recording what it receives.
    let received: Arc<Mutex<Vec<serde_json::Value>>> = Arc::default();
    let platform = {
        let received = received.clone();
        Router::new().route(
            "/api/events/batch",
            post(move |Json(body): Json<serde_json::Value>| {
                let received = received.clone();
                async move {
                    let count = body["items"].as_array().map_or(0, Vec::len);
                    received.lock().unwrap().push(body);
                    Json(serde_json::json!({
                        "results": (0..count)
                            .map(|i| serde_json::json!({"id": format!("evt{i}"), "status": "SUCCESS"}))
                            .collect::<Vec<_>>()
                    }))
                }
            }),
        )
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let platform_port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, platform).await.unwrap() });

    let _processor = Running(
        Command::new(env!("CARGO_BIN_EXE_fc-outbox-processor"))
            .env("FC_OUTBOX_BACKEND", "mysql")
            .env("FC_OUTBOX_DB_URL", &url)
            .env(
                "FC_OUTBOX_PLATFORM_URL",
                format!("http://127.0.0.1:{platform_port}"),
            )
            .env("FC_OUTBOX_PLATFORM_AUTH_TOKEN", "test-token")
            .env("FC_OUTBOX_POLL_INTERVAL_MS", "100")
            .env("FC_METRICS_PORT", free_port().to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );

    // The binary creates the SDK's table on start.
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    let mut created = false;
    for _ in 0..300 {
        let tables: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.tables \
             WHERE table_schema = 'test' AND table_name = 'outbox_messages'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        if tables == 1 {
            created = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(created, "the processor did not create outbox_messages");

    sqlx::query(
        "INSERT INTO outbox_messages (id, type, message_group, payload, status) \
         VALUES ('0HZXEQ5Y8JY5Z', 'EVENT', 'orders:1', '{\"type\":\"orders:order:created\"}', 0)",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Delivered, then deleted as a success.
    let mut remaining = 1;
    for _ in 0..300 {
        remaining = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM outbox_messages")
            .fetch_one(&pool)
            .await
            .unwrap();
        if remaining == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(remaining, 0, "the row was not delivered");
    let received = received.lock().unwrap();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0]["items"][0]["type"], "orders:order:created");
}
