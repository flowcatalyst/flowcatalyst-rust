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
        dispatch_mode_specified: true,
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
    let remaining: i64 = sqlx::query(
        "SELECT COUNT(*) as count FROM queue_messages WHERE queue_name = $1 AND id = $2",
    )
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

// Per-row receipt handle (found while wiring up the router bench,
// bench/router BROKER=postgres): `ack`/`nack` key solely on
// `receipt_handle` + `queue_name` (no `id` column in the WHERE clause), so
// every row a single poll() call claims MUST get its own unique receipt
// handle — sharing one handle across the whole batch means the first
// message to ack deletes every other message's row in the same statement,
// and every other message's own ack/nack then silently affects zero rows.
//
// Pins: acking ONE message out of a 3-message batch deletes exactly that
// row — the other two must still be present and still ackable on their own
// receipt handles afterward.
//
// Mutant check: reverting the claiming UPDATE to a single shared
// `poll_uuid` (no `|| ':' || m.id`) — confirmed by hand while implementing
// this fix — makes the first ack delete all three rows, so the second and
// third assertions below fail (the rows are already gone).
#[tokio::test]
#[ignore = "requires Docker"]
async fn test_batch_claim_gives_each_row_its_own_receipt_handle() {
    let (pool, _container) = setup_pool().await;
    let queue = PostgresQueue::new(pool.clone(), "pg-test-batch".to_string(), 30);
    queue.init_schema().await.unwrap();

    for id in ["m1", "m2", "m3"] {
        queue
            .publish(healthy_message(id))
            .await
            .expect("publish must succeed");
    }

    let messages = queue.poll(10).await.expect("poll must succeed");
    assert_eq!(
        messages.len(),
        3,
        "all three published messages must be claimed"
    );

    let handles: std::collections::HashSet<&str> =
        messages.iter().map(|m| m.receipt_handle.as_str()).collect();
    assert_eq!(
        handles.len(),
        3,
        "each of the three claimed rows must get a distinct receipt handle, \
         not one shared across the whole batch"
    );

    // Ack only the first message.
    queue
        .ack(&messages[0].receipt_handle)
        .await
        .expect("ack of the first message must succeed");

    let remaining: i64 = sqlx::query("SELECT count(*) FROM queue_messages WHERE queue_name = $1")
        .bind("pg-test-batch")
        .fetch_one(&pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(
        remaining, 2,
        "acking one message must remove exactly that row, leaving the \
         other two untouched"
    );

    // The other two messages' own receipt handles must still resolve —
    // proof their rows were never touched by the first ack.
    queue
        .ack(&messages[1].receipt_handle)
        .await
        .expect("second message's own receipt handle must still be valid");
    queue
        .ack(&messages[2].receipt_handle)
        .await
        .expect("third message's own receipt handle must still be valid");

    let remaining: i64 = sqlx::query("SELECT count(*) FROM queue_messages WHERE queue_name = $1")
        .bind("pg-test-batch")
        .fetch_one(&pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(remaining, 0, "all three rows must now be gone");
}

fn grouped_message(id: &str, group: &str) -> Message {
    Message {
        message_group_id: Some(group.to_string()),
        dispatch_mode: DispatchMode::BlockOnError,
        ..healthy_message(id)
    }
}

async fn insert_row(pool: &PgPool, queue_name: &str, msg: &Message, created_at: i64) {
    sqlx::query(
        "INSERT INTO queue_messages (id, queue_name, message_group_id, visible_at, payload, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&msg.id)
    .bind(queue_name)
    .bind(&msg.message_group_id)
    .bind(created_at)
    .bind(serde_json::to_string(msg).unwrap())
    .bind(created_at)
    .execute(pool)
    .await
    .unwrap();
}

/// R4 (Go's claim query): a group head released with a delay blocks its
/// successors until it is visible again. The old windowed query only
/// ranked VISIBLE rows, so the successor became the "head" and overtook it
/// on the very next poll.
#[tokio::test]
#[ignore = "requires Docker"]
async fn delayed_nack_of_group_head_blocks_its_successor() {
    let (pool, _container) = setup_pool().await;
    let queue = PostgresQueue::new(pool.clone(), "pg-test-r4".to_string(), 30);
    queue.init_schema().await.unwrap();

    let now = chrono::Utc::now().timestamp() - 10;
    insert_row(&pool, "pg-test-r4", &grouped_message("head", "g1"), now).await;
    insert_row(&pool, "pg-test-r4", &grouped_message("next", "g1"), now + 1).await;

    let first = queue.poll(10).await.unwrap();
    assert_eq!(
        first
            .iter()
            .map(|m| m.message.id.as_str())
            .collect::<Vec<_>>(),
        vec!["head"],
        "only the group head is eligible"
    );
    queue
        .nack(&first[0].receipt_handle, Some(30))
        .await
        .unwrap();

    let second = queue.poll(10).await.unwrap();
    assert!(
        second.is_empty(),
        "a delayed-nacked head must hold its successor back, got {:?}",
        second.iter().map(|m| &m.message.id).collect::<Vec<_>>()
    );
}

/// Same-second rows of one group break the tie on id, deterministically,
/// as Go's query does. The windowed query ordered by created_at alone, so
/// which of two same-second rows was the "head" was arbitrary.
#[tokio::test]
#[ignore = "requires Docker"]
async fn same_second_group_rows_break_the_tie_on_id() {
    let (pool, _container) = setup_pool().await;
    let queue = PostgresQueue::new(pool.clone(), "pg-test-tie".to_string(), 30);
    queue.init_schema().await.unwrap();

    let now = chrono::Utc::now().timestamp() - 10;
    // Inserted in reverse id order, same created_at.
    for id in ["m-9", "m-5", "m-1"] {
        insert_row(&pool, "pg-test-tie", &grouped_message(id, "g"), now).await;
    }
    for expected in ["m-1", "m-5", "m-9"] {
        let batch = queue.poll(10).await.unwrap();
        assert_eq!(batch.len(), 1, "one head per group per poll");
        assert_eq!(batch[0].message.id, expected);
        queue.ack(&batch[0].receipt_handle).await.unwrap();
    }
}

/// Concurrent pollers on one queue never claim the same row twice
/// (`FOR UPDATE SKIP LOCKED` over the claim set, as Go does).
#[tokio::test]
#[ignore = "requires Docker"]
async fn concurrent_pollers_never_claim_the_same_row() {
    let (pool, _container) = setup_pool().await;
    let queue_name = "pg-test-concurrent".to_string();
    let setup = PostgresQueue::new(pool.clone(), queue_name.clone(), 30);
    setup.init_schema().await.unwrap();
    let now = chrono::Utc::now().timestamp() - 10;
    for i in 0..300 {
        insert_row(
            &pool,
            &queue_name,
            &healthy_message(&format!("c-{i:03}")),
            now,
        )
        .await;
    }

    let mut handles = Vec::new();
    for _ in 0..6 {
        let q = PostgresQueue::new(pool.clone(), queue_name.clone(), 30);
        handles.push(tokio::spawn(async move {
            let mut ids = Vec::new();
            loop {
                let batch = q.poll(10).await.unwrap();
                if batch.is_empty() {
                    break;
                }
                ids.extend(batch.into_iter().map(|m| m.message.id));
            }
            ids
        }));
    }
    let mut all = Vec::new();
    for h in handles {
        all.extend(h.await.unwrap());
    }
    let unique: std::collections::HashSet<_> = all.iter().cloned().collect();
    assert_eq!(all.len(), unique.len(), "a row was claimed twice");
    assert_eq!(unique.len(), 300, "every row must be claimed exactly once");
}

/// Go parity: a release (nack/defer) whose receipt no longer matches a row
/// is not an error, and the counters Go keeps are kept.
#[tokio::test]
#[ignore = "requires Docker"]
async fn stale_release_is_not_an_error_and_counters_track() {
    let (pool, _container) = setup_pool().await;
    let queue = PostgresQueue::new(pool.clone(), "pg-test-counters".to_string(), 30);
    queue.init_schema().await.unwrap();
    queue.publish(healthy_message("k1")).await.unwrap();
    queue.publish(healthy_message("k2")).await.unwrap();

    let batch = queue.poll(10).await.unwrap();
    assert_eq!(batch.len(), 2);
    queue.ack(&batch[0].receipt_handle).await.unwrap();
    queue
        .defer(&batch[1].receipt_handle, Some(0))
        .await
        .unwrap();
    queue
        .nack("no-such-handle", Some(5))
        .await
        .expect("a stale release is not an error (Go: makeVisible)");

    let c = queue.get_counters().expect("counters");
    assert_eq!(c.total_polled, 2);
    assert_eq!(c.total_acked, 1);
    assert_eq!(c.total_deferred, 1);
    assert_eq!(c.total_nacked, 1);
}
