//! The dispatch-job lifecycle against a real PostgreSQL: every transition,
//! from every status, ends where `lifecycle::TRANSITIONS` says (or is
//! refused and leaves the row untouched), every move stamps `updated_at`,
//! and `msg_dispatch_queue` holds exactly one up-to-date row per PENDING job
//! after every operation. Requires Docker, or a local PostgreSQL through
//! `FC_TEST_PG_BIN` (see `support/db.rs`):
//!   cargo test -p fc-platform --test it dispatch_lifecycle_test:: -- --ignored

use std::collections::HashMap;
use std::time::Duration as StdDuration;

use crate::support::{start_db, TestDb};
use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;

use fc_platform::dispatch_job::lifecycle::{self, Transition, TRANSITIONS};
use fc_platform::dispatch_job::DispatchStatus;
use fc_platform::shared::database::{create_pool, run_migrations, MigrationProfile};
use fc_stream::partition_manager::{self, PartitionManagerConfig};
use tokio::time::sleep;

const STATUSES: [&str; 7] = [
    "PENDING",
    "QUEUED",
    "PROCESSING",
    "COMPLETED",
    "FAILED",
    "CANCELLED",
    "EXPIRED",
];

async fn setup_db() -> (PgPool, TestDb) {
    let (container, url) = start_db("fc").await;
    let pool = create_pool(&url).await.expect("connect");
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
         last_attempt_at, completed_at, duration_millis, subscription_id, dispatch_pool_id, \
         client_id, queue, scheduled_for) \
         VALUES ($1, 'app:dom:agg:done', 'http://subscriber.test/hook', $2, $3, $4, $5, $6, $7, \
         2, 'earlier error', $7, $7, $7, 11, 'sub_fixture', 'pool_fixture', 'clt_fixture', \
         'queue_fixture', $7)",
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
    // A raw insert does not go through the lifecycle, so it does not write
    // the queue row: give a PENDING fixture the row the lifecycle would have.
    seed_queue_row(pool, id).await;
}

/// Test-only: the queue row the lifecycle would hold for `id`, if it is
/// PENDING (a fixture inserted by hand bypasses the lifecycle).
async fn seed_queue_row(pool: &PgPool, id: &str) {
    sqlx::query(
        "INSERT INTO msg_dispatch_queue (job_id, job_created_at, message_group, sequence, \
         scheduled_for, subscription_id, dispatch_pool_id, client_id, mode, queue, version) \
         SELECT id, created_at, message_group, sequence, scheduled_for, subscription_id, \
                dispatch_pool_id, client_id, mode, queue, updated_at \
           FROM msg_dispatch_jobs WHERE id = $1 AND status = 'PENDING' \
         ON CONFLICT (job_id) DO NOTHING",
    )
    .bind(id)
    .execute(pool)
    .await
    .expect("seed queue row");
}

/// A queue row, every column.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
struct QueueRow {
    job_id: String,
    job_created_at: DateTime<Utc>,
    message_group: Option<String>,
    sequence: i32,
    scheduled_for: Option<DateTime<Utc>>,
    subscription_id: Option<String>,
    dispatch_pool_id: Option<String>,
    client_id: Option<String>,
    mode: String,
    queue: Option<String>,
    version: DateTime<Utc>,
    claimed_at: Option<DateTime<Utc>>,
    enqueued_at: DateTime<Utc>,
}

async fn queue_row(pool: &PgPool, id: &str) -> Option<QueueRow> {
    sqlx::query_as("SELECT * FROM msg_dispatch_queue WHERE job_id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .unwrap()
}

/// The job columns the queue row mirrors.
#[derive(Debug, PartialEq, sqlx::FromRow)]
struct JobMirror {
    status: String,
    created_at: DateTime<Utc>,
    message_group: Option<String>,
    sequence: i32,
    scheduled_for: Option<DateTime<Utc>>,
    subscription_id: Option<String>,
    dispatch_pool_id: Option<String>,
    client_id: Option<String>,
    mode: String,
    queue: Option<String>,
    updated_at: DateTime<Utc>,
}

/// THE INVARIANT, for one job: a queue row exists iff the job is PENDING,
/// and when it exists it mirrors the job's current values with `claimed_at`
/// unset.
async fn assert_invariant(pool: &PgPool, id: &str, ctx: &str) {
    let job: JobMirror = sqlx::query_as(
        "SELECT status, created_at, message_group, sequence, scheduled_for, subscription_id, \
         dispatch_pool_id, client_id, mode, queue, updated_at \
         FROM msg_dispatch_jobs WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap();
    let q = queue_row(pool, id).await;
    if job.status != "PENDING" {
        assert!(
            q.is_none(),
            "{ctx}: job {id} is {} but has a queue row",
            job.status
        );
        return;
    }
    let q = q.unwrap_or_else(|| panic!("{ctx}: PENDING job {id} has no queue row"));
    assert_eq!(q.version, job.updated_at, "{ctx}: version of {id}");
    assert_eq!(
        q.job_created_at, job.created_at,
        "{ctx}: created_at of {id}"
    );
    assert_eq!(
        q.scheduled_for, job.scheduled_for,
        "{ctx}: scheduled_for of {id}"
    );
    assert_eq!(q.message_group, job.message_group, "{ctx}: group of {id}");
    assert_eq!(q.sequence, job.sequence, "{ctx}: sequence of {id}");
    assert_eq!(
        q.dispatch_pool_id, job.dispatch_pool_id,
        "{ctx}: pool of {id}"
    );
    assert_eq!(q.client_id, job.client_id, "{ctx}: client of {id}");
    assert_eq!(q.subscription_id, job.subscription_id, "{ctx}: sub of {id}");
    assert_eq!(q.mode, job.mode, "{ctx}: mode of {id}");
    assert_eq!(q.queue, job.queue, "{ctx}: queue of {id}");
    assert_eq!(q.claimed_at, None, "{ctx}: claimed_at of {id}");
}

/// The invariant over a set of jobs, and the table-wide drift check.
async fn assert_all_exact(pool: &PgPool, ids: &[String], ctx: &str) {
    for id in ids {
        assert_invariant(pool, id, ctx).await;
    }
    let drift = lifecycle::queue_drift(pool).await.unwrap();
    assert_eq!(
        drift,
        lifecycle::QueueDrift {
            missing_or_stale: 0,
            orphaned: 0
        },
        "{ctx}"
    );
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
            let queue_before = queue_row(&pool, &id).await;
            assert_invariant(&pool, &id, "fixture").await;

            let moved = run(&pool, t, &id, created_at).await;
            let after = row(&pool, &id).await;
            let ctx = format!("{} from {from}", t.name);
            assert_invariant(&pool, &id, &ctx).await;

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
                    assert_eq!(
                        queue_row(&pool, &id).await,
                        queue_before,
                        "{ctx} is refused and must leave the queue row byte-identical"
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

// ─── The queue table ────────────────────────────────────────────────────────
//
// `msg_dispatch_queue` holds one row for every PENDING job and none for any
// other, written by the same statement that moves the job. None of the tests
// below needs Docker (`#[ignore]`); see the module doc.

fn ids_of(prefix: &str, n: usize) -> Vec<String> {
    (0..n).map(|i| format!("{prefix}{i:011}")).collect()
}

fn full_job(
    id: &str,
    group: Option<&str>,
    sequence: i32,
    created_at: DateTime<Utc>,
) -> lifecycle::NewJob {
    lifecycle::NewJob {
        id: id.to_string(),
        external_id: None,
        source: Some("test".to_string()),
        kind: "EVENT".to_string(),
        code: "app:dom:agg:done".to_string(),
        subject: None,
        event_id: None,
        correlation_id: None,
        metadata: serde_json::json!([]),
        target_url: "http://subscriber.test/hook".to_string(),
        protocol: "HTTP_WEBHOOK".to_string(),
        payload: Some("{}".to_string()),
        payload_content_type: "application/json".to_string(),
        data_only: true,
        service_account_id: None,
        client_id: Some("clt_created".to_string()),
        subscription_id: Some("sub_created".to_string()),
        mode: "IMMEDIATE".to_string(),
        dispatch_pool_id: Some("pool_created".to_string()),
        message_group: group.map(str::to_string),
        sequence,
        timeout_seconds: 30,
        schema_id: None,
        max_retries: 3,
        retry_strategy: "exponential".to_string(),
        scheduled_for: Some(created_at + Duration::minutes(5)),
        expires_at: None,
        attempt_count: 0,
        last_attempt_at: None,
        completed_at: None,
        duration_millis: None,
        last_error: None,
        idempotency_key: Some(format!("idem:{id}")),
        created_at,
        updated_at: created_at,
        descriptor: Some("a subscription".to_string()),
        queue: Some("queue_created".to_string()),
    }
}

fn fan_out_job(id: &str, group: Option<&str>, created_at: DateTime<Utc>) -> lifecycle::FanOutJob {
    lifecycle::FanOutJob {
        id: id.to_string(),
        code: "app:dom:agg:done".to_string(),
        source: "test".to_string(),
        subject: None,
        event_id: format!("e{}", &id[1..]),
        correlation_id: None,
        target_url: "http://subscriber.test/hook".to_string(),
        protocol: "HTTP_WEBHOOK",
        payload: "{}".to_string(),
        data_only: true,
        service_account_id: None,
        client_id: Some("clt_fanned".to_string()),
        subscription_id: "sub_fanned".to_string(),
        queue: Some("queue_fanned".to_string()),
        descriptor: Some("a subscription".to_string()),
        metadata: None,
        mode: "BLOCK_ON_ERROR",
        dispatch_pool_id: Some("pool_fanned".to_string()),
        message_group: group.map(str::to_string),
        sequence: 7,
        timeout_seconds: 30,
        max_retries: 3,
        idempotency_key: format!("idem:{id}"),
        created_at,
    }
}

async fn queue_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM msg_dispatch_queue")
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Creation: every inserted job is PENDING with a queue row mirroring it
/// (batch of N gives N rows); a statement that fails on a duplicate id adds
/// and changes nothing; the fan-out insert does the same in its transaction,
/// and a rolled-back fan-out leaves no queue rows.
#[tokio::test]
#[ignore = "requires Docker"]
async fn creation_writes_a_queue_row_for_every_inserted_job() {
    let (pool, _c) = setup_db().await;
    let created_at = Utc::now() - Duration::minutes(3);
    let ids = ids_of("c", 5);
    let jobs: Vec<_> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| full_job(id, (i % 2 == 0).then_some("grp"), i as i32, created_at))
        .collect();
    lifecycle::create_batch(&pool, &jobs).await.unwrap();
    assert_eq!(queue_count(&pool).await, 5);
    assert_all_exact(&pool, &ids, "after create_batch").await;
    for id in &ids {
        assert_eq!(row(&pool, id).await.status, "PENDING");
    }

    // A batch with one duplicate id fails as a whole: the new jobs and their
    // queue rows are not added, the old ones are not changed.
    let before: Vec<_> = futures_rows(&pool, &ids).await;
    let mut again = vec![full_job(&format!("d{:011}", 0), None, 0, created_at)];
    again.push(full_job(&ids[0], None, 0, created_at));
    assert!(lifecycle::create_batch(&pool, &again).await.is_err());
    assert_eq!(queue_count(&pool).await, 5);
    assert_eq!(futures_rows(&pool, &ids).await, before);

    // Fan-out, committed.
    let fan_ids = ids_of("f", 3);
    let fan: Vec<_> = fan_ids
        .iter()
        .map(|id| fan_out_job(id, Some("fgrp"), created_at))
        .collect();
    let mut tx = pool.begin().await.unwrap();
    lifecycle::create_fanned_out(&mut *tx, &fan).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(queue_count(&pool).await, 8);
    assert_all_exact(&pool, &fan_ids, "after create_fanned_out").await;

    // Fan-out, rolled back.
    let rolled = ids_of("r", 2);
    let fan: Vec<_> = rolled
        .iter()
        .map(|id| fan_out_job(id, None, created_at))
        .collect();
    let mut tx = pool.begin().await.unwrap();
    lifecycle::create_fanned_out(&mut *tx, &fan).await.unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(queue_count(&pool).await, 8);
}

async fn futures_rows(pool: &PgPool, ids: &[String]) -> Vec<Option<QueueRow>> {
    let mut out = Vec::new();
    for id in ids {
        out.push(queue_row(pool, id).await);
    }
    out
}

/// Bulk transitions: settled-return of many ids, stale recovery of both
/// kinds, the reaper's sweep and a requeue of many: the invariant holds for
/// every job touched, and the drift check reports nothing.
#[tokio::test]
#[ignore = "requires Docker"]
async fn bulk_transitions_keep_the_queue_exact() {
    let (pool, _c) = setup_db().await;
    let created_at = Utc::now() - Duration::minutes(30);
    let far = Utc::now() + Duration::hours(2);
    let mut all = Vec::new();
    let mut by_status = HashMap::new();
    for (prefix, status) in [
        ("q", "QUEUED"),
        ("p", "PROCESSING"),
        ("f", "FAILED"),
        ("c", "COMPLETED"),
        ("n", "PENDING"),
    ] {
        let ids = ids_of(prefix, 4);
        for id in &ids {
            insert_job(&pool, id, status, None, 1, "IMMEDIATE", created_at).await;
        }
        all.extend(ids.clone());
        by_status.insert(status, ids);
    }
    assert_all_exact(&pool, &all, "fixtures").await;

    // Settled-return: two QUEUED, two PROCESSING, and a COMPLETED (refused).
    let mut ids = by_status["QUEUED"][..2].to_vec();
    ids.extend(by_status["PROCESSING"][..2].to_vec());
    ids.push(by_status["COMPLETED"][0].clone());
    let moved = lifecycle::return_settled(&pool, &ids, "settled")
        .await
        .unwrap();
    assert_eq!(moved.len(), 4);
    assert_all_exact(&pool, &all, "settled return").await;

    // Stale recovery of the rest of QUEUED and of PROCESSING.
    let moved = lifecycle::recover_stale_queued(&pool, far).await.unwrap();
    assert_eq!(moved.len(), 2);
    assert_all_exact(&pool, &all, "stale queued").await;
    let moved = lifecycle::recover_stale_processing(&pool, far, "stale")
        .await
        .unwrap();
    assert_eq!(moved.len(), 2);
    assert_all_exact(&pool, &all, "stale processing").await;

    // Requeue of many, across statuses (including jobs already PENDING).
    let keys: Vec<(String, DateTime<Utc>)> = all.iter().map(|i| (i.clone(), created_at)).collect();
    let moved = lifecycle::requeue(&pool, &keys).await.unwrap();
    assert_eq!(moved.len(), all.len());
    assert_all_exact(&pool, &all, "requeue").await;
    assert_eq!(queue_count(&pool).await, all.len() as i64);

    // The reaper: a FAILED head with three stranded BLOCK_ON_ERROR siblings.
    let head = "xh000000000".to_string();
    insert_job(
        &pool,
        &head,
        "FAILED",
        Some("reap-grp"),
        1,
        "BLOCK_ON_ERROR",
        created_at - Duration::minutes(1),
    )
    .await;
    let siblings = ids_of("x", 3);
    for (i, id) in siblings.iter().enumerate() {
        let status = if i == 0 { "QUEUED" } else { "PROCESSING" };
        insert_job(
            &pool,
            id,
            status,
            Some("reap-grp"),
            2 + i as i32,
            "BLOCK_ON_ERROR",
            created_at,
        )
        .await;
    }
    let mut touched = all.clone();
    touched.push(head);
    touched.extend(siblings.clone());
    let moved = lifecycle::reap_stranded_siblings(&pool, far, "reaped")
        .await
        .unwrap();
    assert_eq!(moved.len(), 3);
    assert_all_exact(&pool, &touched, "reaper sweep").await;
}

/// Mark-QUEUED: marked rows leave the queue; a job that re-entered PENDING
/// since the claim keeps its refreshed queue row; a stale queue row at the
/// claimed version for a job that moved on is removed; one at a different
/// version is left alone.
#[tokio::test]
#[ignore = "requires Docker"]
async fn mark_queued_and_the_queue() {
    let (pool, _c) = setup_db().await;
    let created_at = Utc::now() - Duration::minutes(5);

    // (a) Two PENDING jobs, claimed and marked.
    let a = ids_of("a", 2);
    let mut claimed = Vec::new();
    for id in &a {
        insert_job(&pool, id, "PENDING", None, 1, "IMMEDIATE", created_at).await;
        claimed.push((id.clone(), created_at, row(&pool, id).await.updated_at));
    }
    let marked = lifecycle::mark_queued(&pool, &claimed).await.unwrap();
    assert_eq!(marked.len(), 2);
    for id in &a {
        assert_eq!(row(&pool, id).await.status, "QUEUED");
        assert_invariant(&pool, id, "marked").await;
        assert!(queue_row(&pool, id).await.is_none());
    }

    // (b) Re-entered PENDING since the claim: not marked, row refreshed.
    let b = "b00000000000";
    insert_job(&pool, b, "PENDING", None, 1, "IMMEDIATE", created_at).await;
    let read = row(&pool, b).await.updated_at;
    assert!(
        lifecycle::defer(&pool, b, created_at, Utc::now() + Duration::minutes(1))
            .await
            .unwrap()
    );
    let marked = lifecycle::mark_queued(&pool, &[(b.to_string(), created_at, read)])
        .await
        .unwrap();
    assert!(marked.is_empty());
    let refreshed = queue_row(&pool, b).await.expect("the refreshed row stays");
    assert_ne!(refreshed.version, read);
    assert_invariant(&pool, b, "re-entered").await;

    // (c) A stale queue row at the claimed version for a job that moved on
    // without the queue row being refreshed (written by hand): removed.
    let c = "c00000000000";
    insert_job(&pool, c, "PENDING", None, 1, "IMMEDIATE", created_at).await;
    let read = row(&pool, c).await.updated_at;
    sqlx::query(
        "UPDATE msg_dispatch_jobs SET status = 'COMPLETED', updated_at = NOW() WHERE id = $1",
    )
    .bind(c)
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        queue_row(&pool, c).await.is_some(),
        "the stale row is there"
    );
    let marked = lifecycle::mark_queued(&pool, &[(c.to_string(), created_at, read)])
        .await
        .unwrap();
    assert!(marked.is_empty());
    assert!(
        queue_row(&pool, c).await.is_none(),
        "the stale row is removed"
    );

    // (d) A stale queue row at a DIFFERENT version is left alone.
    let d = "d00000000000";
    insert_job(&pool, d, "PENDING", None, 1, "IMMEDIATE", created_at).await;
    sqlx::query(
        "UPDATE msg_dispatch_jobs SET status = 'COMPLETED', updated_at = NOW() WHERE id = $1",
    )
    .bind(d)
    .execute(&pool)
    .await
    .unwrap();
    let other = Utc::now() - Duration::days(1);
    lifecycle::mark_queued(&pool, &[(d.to_string(), created_at, other)])
        .await
        .unwrap();
    assert!(queue_row(&pool, d).await.is_some());
    // Only the hand-made orphan (d) is left.
    let drift = lifecycle::queue_drift(&pool).await.unwrap();
    assert_eq!(drift.missing_or_stale, 0);
    assert_eq!(drift.orphaned, 1);
}

/// Migration 064 on a database whose jobs predate it: the backfill produces
/// exactly the PENDING ones; running it again is a no-op.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_migration_backfills_exactly_the_pending_jobs() {
    let (pool, _c) = setup_db().await;
    let created_at = Utc::now() - Duration::minutes(5);
    let mut pending = Vec::new();
    for (i, status) in [
        "PENDING",
        "QUEUED",
        "PENDING",
        "FAILED",
        "PROCESSING",
        "PENDING",
        "COMPLETED",
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("m{i:011}");
        insert_job(
            &pool,
            &id,
            status,
            Some("g"),
            i as i32,
            "IMMEDIATE",
            created_at,
        )
        .await;
        if status == "PENDING" {
            pending.push(id);
        }
    }

    // Take the database back to before the migration.
    sqlx::query("DROP TABLE msg_dispatch_queue")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM _schema_migrations WHERE migration_id = '064_dispatch_queue'")
        .execute(&pool)
        .await
        .unwrap();
    run_migrations(&pool, MigrationProfile::Production)
        .await
        .unwrap();

    let mut got: Vec<String> =
        sqlx::query_scalar("SELECT job_id FROM msg_dispatch_queue ORDER BY 1")
            .fetch_all(&pool)
            .await
            .unwrap();
    got.sort();
    assert_eq!(got, pending);
    assert_all_exact(&pool, &pending, "backfilled").await;

    // Re-running the migration's SQL changes nothing.
    let before = futures_rows(&pool, &pending).await;
    sqlx::raw_sql(include_str!(
        "../../../../migrations/064_dispatch_queue.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(futures_rows(&pool, &pending).await, before);
    assert_eq!(queue_count(&pool).await, pending.len() as i64);

    // A tracker that predates it (a database another platform migrated): the
    // backfill probe finds the table's index, records the migration and
    // leaves the rows alone.
    sqlx::query("DELETE FROM _schema_migrations")
        .execute(&pool)
        .await
        .unwrap();
    run_migrations(&pool, MigrationProfile::Production)
        .await
        .unwrap();
    let tracked: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM _schema_migrations \
         WHERE migration_id = '064_dispatch_queue')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(tracked);
    assert_eq!(futures_rows(&pool, &pending).await, before);
}

/// Dropping a job partition removes that partition's queue rows and no
/// others.
#[tokio::test]
#[ignore = "requires Docker"]
async fn dropping_a_partition_removes_its_queue_rows_only() {
    use chrono::TimeZone;
    let (pool, _c) = setup_db().await;
    for (name, from, to) in [
        ("msg_dispatch_jobs_2020_01", "2020-01-01", "2020-02-01"),
        ("msg_dispatch_jobs_2020_02", "2020-02-01", "2020-03-01"),
    ] {
        sqlx::query(&format!(
            "CREATE TABLE {name} PARTITION OF msg_dispatch_jobs \
             FOR VALUES FROM ('{from}') TO ('{to}')"
        ))
        .execute(&pool)
        .await
        .unwrap();
    }
    let jan = Utc.with_ymd_and_hms(2020, 1, 15, 0, 0, 0).unwrap();
    let feb = Utc.with_ymd_and_hms(2020, 2, 15, 0, 0, 0).unwrap();
    let now = Utc::now();
    let mut in_jan = Vec::new();
    let mut kept = Vec::new();
    for (prefix, at, bucket) in [("j", jan, 0), ("e", feb, 1), ("n", now, 1)] {
        for id in ids_of(prefix, 2) {
            insert_job(&pool, &id, "PENDING", None, 1, "IMMEDIATE", at).await;
            if bucket == 0 {
                in_jan.push(id);
            } else {
                kept.push(id);
            }
        }
    }
    assert_eq!(queue_count(&pool).await, 6);

    // Retention such that the cutoff falls mid-February 2020: January's
    // partition is expired, February's is not.
    let days = (now - feb).num_days() as u32;
    let config = PartitionManagerConfig {
        months_forward: 0,
        retention_days: days,
        tick_interval: StdDuration::from_secs(3600),
    };
    partition_manager::tick(&pool, &config).await.unwrap();

    let gone: bool = sqlx::query_scalar("SELECT to_regclass('msg_dispatch_jobs_2020_01') IS NULL")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(gone, "the January partition is dropped");
    for id in &in_jan {
        assert!(queue_row(&pool, id).await.is_none(), "{id}");
    }
    assert_all_exact(&pool, &kept, "after the drop").await;
    assert_eq!(queue_count(&pool).await, 4);
}

/// `queue_drift` reports zero on a healthy table, and reports each kind of
/// corruption a test makes by hand.
#[tokio::test]
#[ignore = "requires Docker"]
async fn queue_drift_reports_what_a_test_corrupts() {
    let (pool, _c) = setup_db().await;
    let created_at = Utc::now() - Duration::minutes(5);
    let ids = ids_of("d", 5);
    for id in &ids {
        insert_job(&pool, id, "PENDING", None, 1, "IMMEDIATE", created_at).await;
    }
    assert_all_exact(&pool, &ids, "healthy").await;

    // Missing: a PENDING job loses its queue row.
    sqlx::query("DELETE FROM msg_dispatch_queue WHERE job_id = $1")
        .bind(&ids[0])
        .execute(&pool)
        .await
        .unwrap();
    // Stale: a row at another version; another at another scheduled_for.
    sqlx::query(
        "UPDATE msg_dispatch_queue SET version = version - INTERVAL '1 second' WHERE job_id = $1",
    )
    .bind(&ids[1])
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE msg_dispatch_queue SET scheduled_for = NULL WHERE job_id = $1")
        .bind(&ids[2])
        .execute(&pool)
        .await
        .unwrap();
    // Orphaned: a job that is no longer PENDING, and a job that does not exist.
    sqlx::query("UPDATE msg_dispatch_jobs SET status = 'COMPLETED' WHERE id = $1")
        .bind(&ids[3])
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO msg_dispatch_queue (job_id, job_created_at, sequence, mode, version) \
         VALUES ('ghost0000000', NOW(), 1, 'IMMEDIATE', NOW())",
    )
    .execute(&pool)
    .await
    .unwrap();

    let drift = lifecycle::queue_drift(&pool).await.unwrap();
    assert_eq!(
        drift,
        lifecycle::QueueDrift {
            missing_or_stale: 3,
            orphaned: 2
        }
    );
}

/// Waits (up to 10 s) until some statement is blocked on a lock.
async fn wait_for_a_blocked_statement(pool: &PgPool) {
    for _ in 0..100 {
        let blocked: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pg_stat_activity \
             WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        if blocked > 0 {
            return;
        }
        sleep(StdDuration::from_millis(100)).await;
    }
    panic!("no statement became blocked on a lock");
}

/// The direction that must not happen: a PENDING job with no queue row. An
/// enter that waits on the job's row lock behind a leave re-inserts with
/// ON CONFLICT, which sees the leave's committed delete.
#[tokio::test]
#[ignore = "requires Docker"]
async fn an_enter_behind_a_leave_cannot_lose_the_job() {
    let (pool, _c) = setup_db().await;
    let created_at = Utc::now() - Duration::minutes(5);
    let id = "i00000000001";
    insert_job(&pool, id, "PENDING", None, 1, "IMMEDIATE", created_at).await;

    // The leave holds the job's row lock, uncommitted.
    let mut tx = pool.begin().await.unwrap();
    assert!(lifecycle::complete(&mut *tx, id, created_at, 1)
        .await
        .unwrap());
    // The enter (operator requeue) blocks behind it.
    let p2 = pool.clone();
    let enter =
        tokio::spawn(async move { lifecycle::requeue(&p2, &[(id.to_string(), created_at)]).await });
    wait_for_a_blocked_statement(&pool).await;
    tx.commit().await.unwrap();
    assert_eq!(enter.await.unwrap().unwrap().len(), 1);

    assert_eq!(row(&pool, id).await.status, "PENDING");
    assert_invariant(&pool, id, "enter behind leave").await;
}

/// The documented, accepted anomaly: a leave that waits on the job's row lock
/// behind an enter cannot see the queue row the enter inserted, so a queue
/// row outlives the job's PENDING status. It is an orphan (never a lost
/// job), and the scheduler's mark-QUEUED removes it.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_leave_behind_an_enter_may_leave_a_harmless_orphan() {
    let (pool, _c) = setup_db().await;
    let created_at = Utc::now() - Duration::minutes(5);
    let id = "i00000000002";
    insert_job(&pool, id, "QUEUED", None, 1, "IMMEDIATE", created_at).await;

    // The enter (a retry) holds the job's row lock, uncommitted, with its
    // queue row written.
    let mut tx = pool.begin().await.unwrap();
    assert!(
        lifecycle::schedule_retry(&mut *tx, id, created_at, Utc::now(), "boom")
            .await
            .unwrap()
    );
    // The leave (a completion) blocks behind it.
    let p2 = pool.clone();
    let leave = tokio::spawn(async move { lifecycle::complete(&p2, id, created_at, 1).await });
    wait_for_a_blocked_statement(&pool).await;
    tx.commit().await.unwrap();
    assert!(leave.await.unwrap().unwrap());

    assert_eq!(row(&pool, id).await.status, "COMPLETED");
    let drift = lifecycle::queue_drift(&pool).await.unwrap();
    // Never a lost job.
    assert_eq!(drift.missing_or_stale, 0);
    // The orphan the race leaves (zero if the delete saw the row after all).
    println!("orphans after the enter/leave race: {}", drift.orphaned);
    assert!(drift.orphaned <= 1);
    if let Some(q) = queue_row(&pool, id).await {
        // Self-healing: mark-QUEUED at the row's version removes it.
        lifecycle::mark_queued(&pool, &[(id.to_string(), created_at, q.version)])
            .await
            .unwrap();
        assert!(queue_row(&pool, id).await.is_none());
    }
    assert_eq!(
        lifecycle::queue_drift(&pool).await.unwrap(),
        lifecycle::QueueDrift {
            missing_or_stale: 0,
            orphaned: 0
        }
    );
}

/// xorshift64: enough randomness for a stress test, no dependency.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// A few thousand random lifecycle operations over a few hundred jobs from
/// several concurrent workers: no PENDING job may end without an up-to-date
/// queue row. Orphans (the documented enter/leave race) are allowed and
/// reported.
#[tokio::test]
#[ignore = "requires Docker"]
async fn random_concurrent_operations_never_lose_a_pending_job() {
    let (pool, _c) = setup_db().await;
    let created_at = Utc::now() - Duration::minutes(30);
    let mut seed = Rng(0x9E37_79B9_7F4A_7C15);
    let ids = ids_of("z", 300);
    for id in &ids {
        let status = STATUSES[seed.below(STATUSES.len())];
        let group = format!("g{}", seed.below(12));
        let mode = if seed.below(2) == 0 {
            "BLOCK_ON_ERROR"
        } else {
            "IMMEDIATE"
        };
        insert_job(
            &pool,
            id,
            status,
            Some(&group),
            seed.below(5) as i32,
            mode,
            created_at,
        )
        .await;
    }

    const WORKERS: u64 = 6;
    const OPS: usize = 700;
    let mut workers = Vec::new();
    for w in 0..WORKERS {
        let pool = pool.clone();
        let ids = ids.clone();
        workers.push(tokio::spawn(async move {
            let mut rng = Rng(0xD1B5_4A32_D192_ED03 ^ (w + 1).wrapping_mul(0x2545_F491_4F6C_DD1D));
            let (mut deadlocks, mut done) = (0usize, 0usize);
            for _ in 0..OPS {
                let id = ids[rng.below(ids.len())].clone();
                let later = Utc::now() + Duration::minutes(rng.below(30) as i64);
                let far = Utc::now() + Duration::hours(2);
                let some_keys = |rng: &mut Rng| -> Vec<(String, DateTime<Utc>)> {
                    (0..1 + rng.below(3))
                        .map(|_| (ids[rng.below(ids.len())].clone(), created_at))
                        .collect()
                };
                let result: Result<(), sqlx::Error> = match rng.below(17) {
                    0 => lifecycle::schedule_retry(&pool, &id, created_at, later, "e")
                        .await
                        .map(drop),
                    1 => lifecycle::defer(&pool, &id, created_at, later)
                        .await
                        .map(drop),
                    2 => lifecycle::hold(&pool, &id, created_at, later)
                        .await
                        .map(drop),
                    3 => {
                        let keys = some_keys(&mut rng);
                        let ids: Vec<String> = keys.into_iter().map(|k| k.0).collect();
                        lifecycle::return_settled(&pool, &ids, "settled")
                            .await
                            .map(drop)
                    }
                    4 => lifecycle::complete(&pool, &id, created_at, 1)
                        .await
                        .map(drop),
                    5 => lifecycle::fail(&pool, &id, created_at, "dead", 1)
                        .await
                        .map(drop),
                    6 | 7 => lifecycle::claim_for_delivery(&pool, &id, created_at)
                        .await
                        .map(drop),
                    8 | 9 => {
                        let version: Option<DateTime<Utc>> = sqlx::query_scalar(
                            "SELECT updated_at FROM msg_dispatch_jobs WHERE id = $1",
                        )
                        .bind(&id)
                        .fetch_optional(&pool)
                        .await
                        .unwrap();
                        let version = version.unwrap();
                        lifecycle::mark_queued(&pool, &[(id.clone(), created_at, version)])
                            .await
                            .map(drop)
                    }
                    10 => lifecycle::requeue(&pool, &some_keys(&mut rng))
                        .await
                        .map(drop),
                    11 => lifecycle::reclaim_stale_delivery(&pool, &id, created_at, far)
                        .await
                        .map(drop),
                    12 => lifecycle::operator_cancel(&pool, &id, created_at)
                        .await
                        .map(drop),
                    13 => lifecycle::operator_complete(&pool, &id, created_at)
                        .await
                        .map(drop),
                    14 => {
                        let cutoff = Utc::now() - Duration::minutes(10);
                        if rng.below(2) == 0 {
                            lifecycle::recover_stale_queued(&pool, cutoff)
                                .await
                                .map(drop)
                        } else {
                            lifecycle::recover_stale_processing(&pool, cutoff, "stale")
                                .await
                                .map(drop)
                        }
                    }
                    15 => lifecycle::reap_stranded_siblings(&pool, far, "reaped")
                        .await
                        .map(drop),
                    _ => lifecycle::mark_queued(
                        &pool,
                        &[(id.clone(), created_at, Utc::now() - Duration::days(1))],
                    )
                    .await
                    .map(drop),
                };
                match result {
                    Ok(()) => done += 1,
                    // Two multi-row statements locking rows in different
                    // orders may deadlock: one is aborted whole (atomic).
                    Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("40P01") => {
                        deadlocks += 1
                    }
                    Err(e) => panic!("operation failed: {e}"),
                }
            }
            (done, deadlocks)
        }));
    }
    let (mut done, mut deadlocks) = (0, 0);
    for w in workers {
        let (d, dl) = w.await.unwrap();
        done += d;
        deadlocks += dl;
    }

    let drift = lifecycle::queue_drift(&pool).await.unwrap();
    println!(
        "random operations: {done} completed, {deadlocks} deadlock-aborted; \
         missing/stale = {}, orphaned = {}",
        drift.missing_or_stale, drift.orphaned
    );
    assert_eq!(
        drift.missing_or_stale, 0,
        "a PENDING job lost its queue row"
    );
}
