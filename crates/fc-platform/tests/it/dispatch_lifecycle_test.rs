//! The dispatch-job lifecycle against a real PostgreSQL: every transition,
//! from every status, ends where `lifecycle::TRANSITIONS` says (or is
//! refused and leaves the row untouched), and every move stamps
//! `updated_at`. Requires Docker (NOT runnable without it; compile-checked
//! only in environments that have none):
//!   cargo test -p fc-platform --test it dispatch_lifecycle_test:: -- --ignored

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;

use fc_platform::dispatch_job::lifecycle::{self, Transition, TRANSITIONS};
use fc_platform::dispatch_job::DispatchStatus;
use fc_platform::shared::database::{create_pool, run_migrations, MigrationProfile};

const STATUSES: [&str; 7] = [
    "PENDING",
    "QUEUED",
    "PROCESSING",
    "COMPLETED",
    "FAILED",
    "CANCELLED",
    "EXPIRED",
];

async fn setup_db() -> (PgPool, ContainerAsync<Postgres>) {
    let container = Postgres::default()
        .with_db_name("fc")
        .with_user("test")
        .with_password("test")
        .start()
        .await
        .expect("start postgres");
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let pool = create_pool(&format!("postgresql://test:test@{host}:{port}/fc"))
        .await
        .expect("connect");
    run_migrations(&pool, MigrationProfile::Production)
        .await
        .expect("migrate");
    (pool, container)
}

/// A job as ingest would leave it, in `status`, last touched an hour ago
/// (so a move's `NOW()` is visibly later) with some history a requeue
/// resets.
async fn insert_job(
    pool: &PgPool,
    id: &str,
    status: &str,
    group: Option<&str>,
    sequence: i32,
    mode: &str,
    created_at: DateTime<Utc>,
) {
    let touched = Utc::now() - Duration::hours(1);
    sqlx::query(
        "INSERT INTO msg_dispatch_jobs (id, code, target_url, status, mode, message_group, \
         sequence, created_at, updated_at, attempt_count, last_error, queued_at, \
         last_attempt_at, completed_at, duration_millis) \
         VALUES ($1, 'app:dom:agg:done', 'http://subscriber.test/hook', $2, $3, $4, $5, $6, $7, \
         2, 'earlier error', $7, $7, $7, 11)",
    )
    .bind(id)
    .bind(status)
    .bind(mode)
    .bind(group)
    .bind(sequence)
    .bind(created_at)
    .bind(touched)
    .execute(pool)
    .await
    .expect("insert job");
}

/// Everything a transition may change, for before/after comparison.
#[derive(Debug, PartialEq, sqlx::FromRow)]
struct Row {
    status: String,
    updated_at: DateTime<Utc>,
    attempt_count: i32,
    last_error: Option<String>,
    scheduled_for: Option<DateTime<Utc>>,
    queued_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    duration_millis: Option<i64>,
}

async fn row(pool: &PgPool, id: &str) -> Row {
    sqlx::query_as(
        "SELECT status, updated_at, attempt_count, last_error, scheduled_for, queued_at, \
         completed_at, duration_millis FROM msg_dispatch_jobs WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap()
}

fn to_status(s: &str) -> DispatchStatus {
    match s {
        "PENDING" => DispatchStatus::Pending,
        "QUEUED" => DispatchStatus::Queued,
        "PROCESSING" => DispatchStatus::Processing,
        "COMPLETED" => DispatchStatus::Completed,
        "FAILED" => DispatchStatus::Failed,
        "CANCELLED" => DispatchStatus::Cancelled,
        "EXPIRED" => DispatchStatus::Expired,
        other => panic!("unknown status {other}"),
    }
}

/// Runs the operation named `t.name` against the job `id` (created at
/// `created_at`, last touched `touched`); `true` when the lifecycle reports
/// it moved a row.
async fn run(pool: &PgPool, t: &Transition, id: &str, created_at: DateTime<Utc>) -> bool {
    let later = Utc::now() + Duration::minutes(10);
    let far = Utc::now() + Duration::hours(2);
    let ids = vec![id.to_string()];
    match t.name {
        "retry" => lifecycle::schedule_retry(pool, id, created_at, later, "boom")
            .await
            .unwrap(),
        "defer" => lifecycle::defer(pool, id, created_at, later).await.unwrap(),
        "hold" => lifecycle::hold(pool, id, created_at, later).await.unwrap(),
        "settle_return" => !lifecycle::return_settled(pool, &ids, "settled")
            .await
            .unwrap()
            .is_empty(),
        "reap" => !lifecycle::reap_stranded_siblings(pool, far, "reaped")
            .await
            .unwrap()
            .iter()
            .all(|r| r.id != id),
        "stale_queued" => lifecycle::recover_stale_queued(pool, far)
            .await
            .unwrap()
            .iter()
            .any(|r| r.id == id),
        "stale_processing" => lifecycle::recover_stale_processing(pool, far, "stale")
            .await
            .unwrap()
            .iter()
            .any(|r| r.id == id),
        "requeue" => !lifecycle::requeue(pool, &[(id.to_string(), created_at)])
            .await
            .unwrap()
            .is_empty(),
        "mark_queued" => {
            let version = row(pool, id).await.updated_at;
            !lifecycle::mark_queued(pool, &[(id.to_string(), created_at, version)])
                .await
                .unwrap()
                .is_empty()
        }
        "claim" => lifecycle::claim_for_delivery(pool, id, created_at)
            .await
            .unwrap(),
        "complete" => lifecycle::complete(pool, id, created_at, 42).await.unwrap(),
        "fail" => lifecycle::fail(pool, id, created_at, "dead", 42)
            .await
            .unwrap(),
        "reclaim" => lifecycle::reclaim_stale_delivery(pool, id, created_at, far)
            .await
            .unwrap(),
        "operator_cancel" => lifecycle::operator_cancel(pool, id, created_at)
            .await
            .unwrap(),
        "operator_complete" => lifecycle::operator_complete(pool, id, created_at)
            .await
            .unwrap(),
        other => panic!("transition {other} has no case in this test"),
    }
}

/// The table-driven test: for every (transition, from status) the job ends
/// in the status the lifecycle table says, or (refused) is not touched at
/// all. A job in a settled status is never changed by anything but the
/// operator requeue.
#[tokio::test]
#[ignore = "requires Docker"]
async fn every_transition_from_every_status_matches_the_table() {
    let (pool, _c) = setup_db().await;
    let mut n = 0;
    for t in TRANSITIONS {
        for from in STATUSES {
            n += 1;
            let id = format!("t{n:012}");
            let created_at = Utc::now() - Duration::minutes(30);
            // The reaper needs a FAILED head ahead of a BLOCK_ON_ERROR
            // sibling in the same group; every other transition ignores it.
            let (group, mode) = if t.name == "reap" {
                let head = format!("h{n:012}");
                insert_job(
                    &pool,
                    &head,
                    "FAILED",
                    Some(&id),
                    1,
                    "BLOCK_ON_ERROR",
                    created_at - Duration::minutes(1),
                )
                .await;
                (Some(id.as_str()), "BLOCK_ON_ERROR")
            } else {
                (None, "IMMEDIATE")
            };
            insert_job(&pool, &id, from, group, 2, mode, created_at).await;
            let before = row(&pool, &id).await;

            let moved = run(&pool, t, &id, created_at).await;
            let after = row(&pool, &id).await;

            match t.apply(to_status(from)) {
                Some(to) => {
                    assert!(moved, "{} from {from} should move the job", t.name);
                    assert_eq!(after.status, to.as_str(), "{} from {from}", t.name);
                    assert!(
                        after.updated_at > before.updated_at,
                        "{} from {from} must stamp updated_at",
                        t.name
                    );
                }
                None => {
                    assert!(!moved, "{} from {from} must be refused", t.name);
                    assert_eq!(
                        after, before,
                        "{} from {from} is refused and must leave the row untouched",
                        t.name
                    );
                }
            }
        }
    }
}

/// A late callback never overwrites a settled job.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_late_callback_does_not_overwrite_a_settled_job() {
    let (pool, _c) = setup_db().await;
    let created_at = Utc::now() - Duration::minutes(5);
    for (i, settled) in ["COMPLETED", "FAILED", "CANCELLED", "EXPIRED"]
        .into_iter()
        .enumerate()
    {
        let id = format!("s{i:012}");
        insert_job(&pool, &id, settled, None, 1, "IMMEDIATE", created_at).await;
        let before = row(&pool, &id).await;
        let later = Utc::now() + Duration::minutes(1);

        assert!(!lifecycle::complete(&pool, &id, created_at, 1)
            .await
            .unwrap());
        assert!(!lifecycle::fail(&pool, &id, created_at, "x", 1)
            .await
            .unwrap());
        assert!(
            !lifecycle::schedule_retry(&pool, &id, created_at, later, "x")
                .await
                .unwrap()
        );
        assert!(!lifecycle::defer(&pool, &id, created_at, later)
            .await
            .unwrap());
        assert!(!lifecycle::hold(&pool, &id, created_at, later)
            .await
            .unwrap());
        assert!(!lifecycle::claim_for_delivery(&pool, &id, created_at)
            .await
            .unwrap());

        assert_eq!(row(&pool, &id).await, before, "{settled} must be untouched");
    }
}

/// Operator requeue resets the job from every status: PENDING, a fresh
/// budget, history cleared.
#[tokio::test]
#[ignore = "requires Docker"]
async fn requeue_from_every_status_ends_pending_with_a_fresh_budget() {
    let (pool, _c) = setup_db().await;
    let created_at = Utc::now() - Duration::minutes(5);
    for (i, from) in STATUSES.into_iter().enumerate() {
        let id = format!("r{i:012}");
        insert_job(&pool, &id, from, None, 1, "IMMEDIATE", created_at).await;
        let moved = lifecycle::requeue(&pool, &[(id.clone(), created_at)])
            .await
            .unwrap();
        assert_eq!(moved.len(), 1, "{from}");
        let after = row(&pool, &id).await;
        assert_eq!(after.status, "PENDING", "{from}");
        assert_eq!(after.attempt_count, 0);
        assert_eq!(after.last_error, None);
        assert_eq!(after.scheduled_for, None);
        assert_eq!(after.queued_at, None);
        assert_eq!(after.completed_at, None);
        assert_eq!(after.duration_millis, None);
    }
}

/// Operator cancel and complete move a FAILED job only; every other status
/// is left alone (the check is in the statement).
#[tokio::test]
#[ignore = "requires Docker"]
async fn operator_cancel_and_complete_move_failed_jobs_only() {
    let (pool, _c) = setup_db().await;
    let created_at = Utc::now() - Duration::minutes(5);
    for (i, from) in STATUSES.into_iter().enumerate() {
        let id = format!("o{i:012}");
        insert_job(&pool, &id, from, None, 1, "IMMEDIATE", created_at).await;
        let before = row(&pool, &id).await;
        let cancelled = lifecycle::operator_cancel(&pool, &id, created_at)
            .await
            .unwrap();
        let completed = lifecycle::operator_complete(&pool, &id, created_at)
            .await
            .unwrap();
        if from == "FAILED" {
            // The first call settled it; the second finds CANCELLED.
            assert!(cancelled && !completed);
            assert_eq!(row(&pool, &id).await.status, "CANCELLED");
        } else {
            assert!(!cancelled && !completed, "{from}");
            assert_eq!(row(&pool, &id).await, before, "{from}");
        }
    }
}

/// The scheduler's optimistic version check: a job the callback touched
/// since the claim is not marked QUEUED.
#[tokio::test]
#[ignore = "requires Docker"]
async fn mark_queued_needs_the_version_the_claim_read() {
    let (pool, _c) = setup_db().await;
    let created_at = Utc::now() - Duration::minutes(5);
    insert_job(
        &pool,
        "v000000000001",
        "PENDING",
        None,
        1,
        "IMMEDIATE",
        created_at,
    )
    .await;
    let read = row(&pool, "v000000000001").await.updated_at;

    // A callback re-pends it (a deferral): a new version.
    assert!(lifecycle::defer(
        &pool,
        "v000000000001",
        created_at,
        Utc::now() + Duration::minutes(1)
    )
    .await
    .unwrap());

    let marked = lifecycle::mark_queued(&pool, &[("v000000000001".to_string(), created_at, read)])
        .await
        .unwrap();
    assert!(marked.is_empty());
    assert_eq!(row(&pool, "v000000000001").await.status, "PENDING");
}
