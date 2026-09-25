//! The event fan-out against a real PostgreSQL (pipeline review H7): it
//! never stamps events fanned-out while it has never loaded the
//! subscriptions, and a raised job carries its subscription's queue
//! priority (Go fan-out R2). Requires Docker:
//!   cargo test -p fc-platform --test dispatch_fan_out_test -- --ignored

use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use tokio_util::sync::CancellationToken;

use fc_platform::shared::database::{create_pool, run_migrations, MigrationProfile};
use fc_stream::health::StreamHealth;
use fc_stream::EventFanOutConfig;

async fn run_for(pool: &PgPool, how_long: Duration) {
    let cancel = CancellationToken::new();
    let task = tokio::spawn(fc_stream::event_fan_out::run(
        pool.clone(),
        EventFanOutConfig {
            batch_size: 200,
            subscription_refresh: Duration::from_millis(100),
        },
        Arc::new(StreamHealth::new("event-fan-out".into())),
        cancel.clone(),
    ));
    tokio::time::sleep(how_long).await;
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .expect("fan-out stops when cancelled")
        .unwrap();
}

async fn fanned_out(pool: &PgPool, id: &str) -> bool {
    sqlx::query_scalar("SELECT fanned_out_at IS NOT NULL FROM msg_events WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn fan_out_waits_for_its_first_subscription_load() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    let container = Postgres::default()
        .with_db_name("fc")
        .with_user("test")
        .with_password("test")
        .start()
        .await
        .unwrap();
    let url = format!(
        "postgresql://test:test@{}:{}/fc",
        container.get_host().await.unwrap(),
        container.get_host_port_ipv4(5432).await.unwrap()
    );
    let pool = create_pool(&url).await.unwrap();
    run_migrations(&pool, MigrationProfile::Production)
        .await
        .unwrap();

    sqlx::query(
        "INSERT INTO msg_subscriptions (id, code, name, target, queue, status, mode) \
         VALUES ('sub_orders', 'orders', 'Orders', 'http://subscriber.test/hook', 'HIGH_PRIORITY', 'ACTIVE', 'BLOCK_ON_ERROR')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO msg_subscription_event_types (subscription_id, event_type_code) \
         VALUES ('sub_orders', 'app:orders:order:shipped')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO msg_events (id, type, source, time, data, message_group) \
         VALUES ('evt000000001', 'app:orders:order:shipped', 'app', NOW(), '{\"orderId\":7}', 'orders-7')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // The subscription load fails (its junction table is gone): the event
    // must stay un-fanned, not be stamped with no jobs.
    sqlx::query("ALTER TABLE msg_subscription_event_types RENAME TO mset_hidden")
        .execute(&pool)
        .await
        .unwrap();
    run_for(&pool, Duration::from_millis(1500)).await;
    assert!(
        !fanned_out(&pool, "evt000000001").await,
        "the event was lost"
    );
    let (jobs,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM msg_dispatch_jobs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(jobs, 0);

    // Once the subscriptions load, the event fans out to a PENDING job that
    // carries the subscription's priority.
    sqlx::query("ALTER TABLE mset_hidden RENAME TO msg_subscription_event_types")
        .execute(&pool)
        .await
        .unwrap();
    run_for(&pool, Duration::from_millis(1500)).await;
    assert!(fanned_out(&pool, "evt000000001").await);
    let (status, queue, mode): (String, Option<String>, String) = sqlx::query_as(
        "SELECT status, queue, mode FROM msg_dispatch_jobs WHERE event_id = 'evt000000001'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "PENDING");
    assert_eq!(queue.as_deref(), Some("HIGH_PRIORITY"));
    assert_eq!(mode, "BLOCK_ON_ERROR");
}
