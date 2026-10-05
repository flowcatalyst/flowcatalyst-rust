//! The dispatch-job lifecycle against a real PostgreSQL: every transition,
//! from every status, ends where `lifecycle::TRANSITIONS` says (or is
//! refused and leaves the row untouched), every move stamps `updated_at`,
//! and a statement that pins its rows by primary key does not lose to a
//! concurrent re-enter. Requires Docker, or a local PostgreSQL through
//! `FC_TEST_PG_BIN` (see `support/db.rs`):
//!   cargo test -p fc-platform --test it dispatch_lifecycle_test:: -- --ignored

use std::time::Duration as StdDuration;

use crate::support::{start_db, TestDb};
use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;

use fc_platform::dispatch_job::lifecycle::{self, Transition, TRANSITIONS};
use fc_platform::dispatch_job::DispatchStatus;
use fc_platform::shared::database::{create_pool, run_migrations, MigrationProfile};
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

fn ids_of(prefix: &str, n: usize) -> Vec<String> {
    (0..n).map(|i| format!("{prefix}{i:011}")).collect()
}

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

// ─── mark-QUEUED against a concurrent re-enter ──────────────────────────────

/// mark-QUEUED pins its rows by primary key and the version the claim read.
/// A re-enter (a retry) that holds the job's lock, uncommitted, makes the
/// statement wait; once it commits the job's version has moved on, so
/// nothing is marked and the job is PENDING at its new version.
#[tokio::test]
#[ignore = "requires Docker"]
async fn mark_queued_waits_for_a_concurrent_re_enter_and_then_marks_nothing() {
    let (pool, _c) = setup_db().await;
    let created_at = Utc::now() - Duration::minutes(5);
    let id = "m00000000001";
    insert_job(&pool, id, "PENDING", None, 1, "IMMEDIATE", created_at).await;
    let claimed_version = row(&pool, id).await.updated_at;

    let mut tx = pool.begin().await.unwrap();
    assert!(
        lifecycle::defer(&mut *tx, id, created_at, Utc::now() + Duration::minutes(1))
            .await
            .unwrap()
    );
    let p2 = pool.clone();
    let mark = tokio::spawn(async move {
        lifecycle::mark_queued(&p2, &[(id.to_string(), created_at, claimed_version)]).await
    });
    wait_for_a_blocked_statement(&pool).await;
    tx.commit().await.unwrap();
    assert_eq!(
        mark.await.unwrap().unwrap().len(),
        0,
        "the job's version moved on: nothing is marked"
    );
    let after = row(&pool, id).await;
    assert_eq!(after.status, "PENDING");
    assert_ne!(after.updated_at, claimed_version);
}

/// The reaper decides in its CTE (from the statement's snapshot) that a
/// sibling is stranded; the guard that must hold on the CURRENT row, the
/// PROCESSING age test, has to be in the UPDATE's own WHERE. A QUEUED sibling
/// behind a FAILED head is claimed for delivery by a callback whose
/// transaction is still open; the sweep blocks on the row lock; once the
/// callback commits the sibling is PROCESSING with a fresh `updated_at`, and
/// the sweep must leave it alone.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_reaper_does_not_reset_a_sibling_a_callback_claimed_while_it_waited() {
    let (pool, _c) = setup_db().await;
    let created_at = Utc::now() - Duration::minutes(30);
    insert_job(
        &pool,
        "h00000000001",
        "FAILED",
        Some("g"),
        1,
        "BLOCK_ON_ERROR",
        created_at - Duration::minutes(1),
    )
    .await;
    let id = "s00000000001";
    insert_job(
        &pool,
        id,
        "QUEUED",
        Some("g"),
        2,
        "BLOCK_ON_ERROR",
        created_at,
    )
    .await;

    let mut tx = pool.begin().await.unwrap();
    assert!(lifecycle::claim_for_delivery(&mut *tx, id, created_at)
        .await
        .unwrap());
    let p2 = pool.clone();
    let sweep = tokio::spawn(async move {
        lifecycle::reap_stranded_siblings(&p2, Utc::now() - Duration::minutes(10), "reaped").await
    });
    wait_for_a_blocked_statement(&pool).await;
    tx.commit().await.unwrap();
    let swept = sweep.await.unwrap().unwrap();
    assert!(
        swept.iter().all(|t| t.id != id),
        "a job being delivered was swept: {swept:?}"
    );
    assert_eq!(row(&pool, id).await.status, "PROCESSING");
}

/// mark-QUEUED marks exactly the pairs whose job is PENDING at the claimed
/// version, in one statement, whatever else is in the batch.
#[tokio::test]
#[ignore = "requires Docker"]
async fn mark_queued_marks_exactly_the_pairs_pending_at_the_claimed_version() {
    let (pool, _c) = setup_db().await;
    let created_at = Utc::now() - Duration::minutes(5);
    let ids = ids_of("k", 5);
    for id in &ids {
        insert_job(&pool, id, "PENDING", None, 1, "IMMEDIATE", created_at).await;
    }
    let mut batch = Vec::new();
    for id in &ids {
        batch.push((id.clone(), created_at, row(&pool, id).await.updated_at));
    }
    // ids[1] completes, ids[2] re-enters (new version), ids[3] has a wrong
    // created_at in the batch (no such job), ids[4] is claimed at a stale
    // version.
    assert!(lifecycle::complete(&pool, &ids[1], created_at, 1)
        .await
        .unwrap());
    assert!(lifecycle::defer(&pool, &ids[2], created_at, Utc::now())
        .await
        .unwrap());
    batch[3].1 = created_at - Duration::days(1);
    batch[4].2 -= Duration::seconds(1);

    let marked = lifecycle::mark_queued(&pool, &batch).await.unwrap();
    let got: Vec<&str> = marked.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(got, vec![ids[0].as_str()]);
    assert_eq!(row(&pool, &ids[0]).await.status, "QUEUED");
    assert_eq!(row(&pool, &ids[2]).await.status, "PENDING");
    assert_eq!(row(&pool, &ids[3]).await.status, "PENDING");
    assert_eq!(row(&pool, &ids[4]).await.status, "PENDING");
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
/// several concurrent workers: no deadlock-free operation may fail, and every
/// job ends in a valid state.
#[tokio::test]
#[ignore = "requires Docker"]
async fn random_concurrent_operations_leave_every_job_in_a_valid_state() {
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

    println!("random operations: {done} completed, {deadlocks} deadlock-aborted");
    // Every job is still there, in a status the lifecycle can produce, and no
    // PENDING job carries a completion stamp it should not.
    let bad: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM msg_dispatch_jobs WHERE id LIKE 'z%' \
         AND (status NOT IN ('PENDING', 'QUEUED', 'PROCESSING', 'COMPLETED', 'FAILED', \
                             'CANCELLED', 'EXPIRED') \
              OR (status = 'PENDING' AND queued_at IS NOT NULL))",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        bad, 0,
        "a job ended in a state the lifecycle cannot produce"
    );
    let total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM msg_dispatch_jobs WHERE id LIKE 'z%'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(total, 300);
}
