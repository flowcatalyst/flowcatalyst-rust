//! PostgreSQL Integration Tests (R-17 / A-07 quarantine)
//!
//! Full-stack tests against a real PostgreSQL instance (via testcontainers)
//! for `PostgresQueue`'s malformed-payload quarantine behaviour: a poison
//! row must not abort the poll batch, must not re-claim forever, and must
//! land in `queue_messages_failed` with the latest failure winning on a
//! repeat.
//!
//! These tests require Docker to be running. They are ignored by default:
//!   cargo test -p fc-queue --features postgres --test postgres_integration_tests -- --ignored

#![cfg(feature = "postgres")]

use sqlx::{PgPool, Row};
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

use fc_common::{DispatchMode, MediationType, Message};
use fc_queue::postgres::PostgresQueue;
use fc_queue::{EmbeddedQueue, QueueConsumer, QueuePublisher};

/// Start a PostgreSQL testcontainer and return a pool connected to it.
async fn setup_pool() -> (PgPool, testcontainers::ContainerAsync<Postgres>) {
    let container = Postgres::default()
        .with_db_name("fc_queue_test")
        .with_user("test")
        .with_password("test")
        .start()
        .await
        .expect("failed to start postgres container");

    let host = container.get_host().await.expect("failed to get host");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("failed to get port");

    let database_url = format!("postgresql://test:test@{host}:{port}/fc_queue_test");

    let pool = PgPool::connect(&database_url)
        .await
        .expect("failed to connect to test database");

    (pool, container)
}

fn healthy_message(id: &str) -> Message {
    Message {
        id: id.to_string(),
        pool_code: "TEST".to_string(),
        auth_token: None,
        signing_secret: None,
        mediation_type: MediationType::HTTP,
        mediation_target: "http://localhost:8080".to_string(),
        message_group_id: None,
        high_priority: false,
        dispatch_mode: DispatchMode::default(),
    }
}

async fn insert_poison_row(pool: &PgPool, queue_name: &str, id: &str, payload: &str) {
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "INSERT INTO queue_messages (id, queue_name, message_group_id, visible_at, payload, created_at) \
         VALUES ($1, $2, NULL, $3, $4, $5)",
    )
    .bind(id)
    .bind(queue_name)
    .bind(now)
    .bind(payload)
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
}

struct FailedRow {
    payload: String,
    error_message: String,
}

async fn get_failed_rows(pool: &PgPool, queue_name: &str, id: &str) -> Vec<FailedRow> {
    sqlx::query(
        "SELECT payload, error_message FROM queue_messages_failed WHERE queue_name = $1 AND id = $2",
    )
    .bind(queue_name)
    .bind(id)
    .fetch_all(pool)
    .await
    .unwrap()
    .into_iter()
    .map(|row| FailedRow {
        payload: row.get("payload"),
        error_message: row.get("error_message"),
    })
    .collect()
}

// R-17 / A-07: a malformed payload must not abort the whole poll batch (the
// claiming UPDATE...RETURNING has already committed by the time the payload
// is parsed), and must not keep re-claiming forever — it is quarantined to
// `queue_messages_failed` and the poll continues.
#[tokio::test]
#[ignore = "requires Docker"]
async fn test_poison_row_quarantined_healthy_row_still_delivers() {
    let (pool, _container) = setup_pool().await;
    let queue = PostgresQueue::new(pool.clone(), "pg-test-queue".to_string(), 30);
    queue.init_schema().await.unwrap();

    queue.publish(healthy_message("healthy-1")).await.unwrap();
    insert_poison_row(&pool, "pg-test-queue", "poison-1", "not-valid-json-at-all").await;

    // The healthy row still delivers in the SAME poll batch; the poison row
    // never fails the poll via `?`.
    let messages = queue.poll(10).await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].message.id, "healthy-1");

    // The poison row landed in queue_messages_failed with its error.
    let failed = get_failed_rows(&pool, "pg-test-queue", "poison-1").await;
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].payload, "not-valid-json-at-all");
    assert!(
        !failed[0].error_message.is_empty(),
        "expected a non-empty decode error to be recorded"
    );

    // Gone from queue_messages, so it can never re-claim forever.
    let remaining: i64 =
        sqlx::query("SELECT COUNT(*) as count FROM queue_messages WHERE queue_name = $1 AND id = $2")
            .bind("pg-test-queue")
            .bind("poison-1")
            .fetch_one(&pool)
            .await
            .unwrap()
            .get("count");
    assert_eq!(remaining, 0);

    // A second poll never sees the poison row again.
    let messages = queue.poll(10).await.unwrap();
    assert!(messages.is_empty());
}

// A-07: a second failure for the same id overwrites the first — the latest
// failure wins, not the first.
#[tokio::test]
#[ignore = "requires Docker"]
async fn test_quarantine_latest_failure_wins() {
    let (pool, _container) = setup_pool().await;
    let queue = PostgresQueue::new(pool.clone(), "pg-test-queue-2".to_string(), 30);
    queue.init_schema().await.unwrap();

    insert_poison_row(&pool, "pg-test-queue-2", "poison-2", "totally not json").await;
    let messages = queue.poll(10).await.unwrap();
    assert!(messages.is_empty());

    let failed = get_failed_rows(&pool, "pg-test-queue-2", "poison-2").await;
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].payload, "totally not json");
    let first_error = failed[0].error_message.clone();

    // Requeue the same id with a payload that fails for a *different*
    // reason (valid JSON, but missing required Message fields) so the two
    // failure reasons are distinguishable.
    insert_poison_row(&pool, "pg-test-queue-2", "poison-2", "{}").await;
    let messages = queue.poll(10).await.unwrap();
    assert!(messages.is_empty());

    let failed = get_failed_rows(&pool, "pg-test-queue-2", "poison-2").await;
    assert_eq!(
        failed.len(),
        1,
        "a repeat failure must overwrite, not accumulate, rows for the same id"
    );
    assert_eq!(failed[0].payload, "{}");
    assert_ne!(
        failed[0].error_message, first_error,
        "the latest failure's reason must replace the earlier one"
    );
}
