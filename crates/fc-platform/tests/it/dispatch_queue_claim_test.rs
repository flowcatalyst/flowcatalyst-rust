//! The dispatch queue table as the scheduler reads it, against a real
//! PostgreSQL: the claim (order, what it skips, concurrent claims), its
//! release, the hold-back (claim time and delivery time), the stale-claim
//! release, the backlog, the reconcile sweep, and migration 065. Requires
//! Docker, or a local PostgreSQL through `FC_TEST_PG_BIN` (see
//! `support/db.rs`):
//!   cargo test -p fc-platform --test it dispatch_queue_claim_test:: -- --ignored

use std::collections::HashSet;

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;

use crate::support::{start_db, TestDb};
use fc_platform::dispatch_job::lifecycle::{self, ReconcileGuards, Reconciled};
use fc_platform::shared::database::{create_pool, run_migrations, MigrationProfile};

async fn setup_db() -> (PgPool, TestDb) {
    let (container, url) = start_db("fc").await;
    let pool = create_pool(&url).await.expect("connect");
    run_migrations(&pool, MigrationProfile::Production)
        .await
        .expect("migrate");
    (pool, container)
}

/// One job row, as the scheduler's tests need it.
#[derive(Clone)]
struct Spec {
    id: String,
    status: &'static str,
    group: Option<&'static str>,
    sequence: i32,
    mode: &'static str,
    created_at: DateTime<Utc>,
    scheduled_for: Option<DateTime<Utc>>,
    subscription_id: Option<&'static str>,
    updated_at: DateTime<Utc>,
}

fn spec(n: i64) -> Spec {
    Spec {
        id: format!("q{n:012}"),
        status: "PENDING",
        group: None,
        sequence: 1,
        mode: "IMMEDIATE",
        // Distinct, ordered creation times.
        created_at: Utc::now() - Duration::seconds(1000 - n),
        scheduled_for: None,
        subscription_id: None,
        updated_at: Utc::now() - Duration::hours(1),
    }
}

/// Inserts the job by hand and, when it is PENDING, the queue row the
/// lifecycle would hold for it.
async fn put(pool: &PgPool, s: &Spec) {
    sqlx::query(
        "INSERT INTO msg_dispatch_jobs (id, code, target_url, status, mode, message_group, \
         sequence, created_at, updated_at, scheduled_for, subscription_id) \
         VALUES ($1, 'app:dom:agg:done', 'http://subscriber.test/hook', $2, $3, $4, $5, $6, $7, \
         $8, $9)",
    )
    .bind(&s.id)
    .bind(s.status)
    .bind(s.mode)
    .bind(s.group)
    .bind(s.sequence)
    .bind(s.created_at)
    .bind(s.updated_at)
    .bind(s.scheduled_for)
    .bind(s.subscription_id)
    .execute(pool)
    .await
    .expect("insert job");
    if s.status == "PENDING" {
        sqlx::query(
            "INSERT INTO msg_dispatch_queue (job_id, job_created_at, message_group, sequence, \
             scheduled_for, subscription_id, mode, version) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(&s.id)
        .bind(s.created_at)
        .bind(s.group)
        .bind(s.sequence)
        .bind(s.scheduled_for)
        .bind(s.subscription_id)
        .bind(s.mode)
        .bind(s.updated_at)
        .execute(pool)
        .await
        .expect("insert queue row");
    }
}

fn ids(rows: &[lifecycle::QueueClaim]) -> Vec<String> {
    rows.iter().map(|r| r.job_id.clone()).collect()
}

async fn claimed_at(pool: &PgPool, id: &str) -> Option<DateTime<Utc>> {
    sqlx::query_scalar("SELECT claimed_at FROM msg_dispatch_queue WHERE job_id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn queue_ids(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT job_id FROM msg_dispatch_queue ORDER BY job_id")
        .fetch_all(pool)
        .await
        .unwrap()
}

// ── The claim ───────────────────────────────────────────────────────────

/// The claim returns rows in the total order (group, NULLS LAST; sequence;
/// created_at; id); skips claimed, not-yet-due and paused rows; does not
/// return a claimed row again until it is released or re-enters PENDING.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_claim_returns_rows_in_order_and_skips_claimed_not_due_and_paused() {
    let (pool, _c) = setup_db().await;
    let mut a = spec(1);
    a.group = Some("b");
    let mut b = spec(2);
    b.group = Some("a");
    b.sequence = 2;
    let mut c = spec(3);
    c.group = Some("a");
    let d = spec(4); // ungrouped: NULLS LAST
    let mut e = spec(5);
    e.group = Some("a");
    e.sequence = 3;
    e.scheduled_for = Some(Utc::now() + Duration::hours(1)); // not due
    let mut f = spec(6);
    f.group = Some("c");
    f.subscription_id = Some("sub_paused");
    let mut g = spec(7);
    g.group = Some("a");
    g.sequence = 4; // already claimed
    let mut done = spec(8);
    done.status = "QUEUED"; // not in the queue at all
    for s in [&a, &b, &c, &d, &e, &f, &g, &done] {
        put(&pool, s).await;
    }
    sqlx::query("UPDATE msg_dispatch_queue SET claimed_at = NOW() WHERE job_id = $1")
        .bind(&g.id)
        .execute(&pool)
        .await
        .unwrap();

    let paused = vec!["sub_paused".to_string()];
    let first = lifecycle::claim(&pool, 100, &paused).await.unwrap();
    assert_eq!(
        ids(&first),
        vec![c.id.clone(), b.id.clone(), a.id.clone(), d.id.clone()],
        "group a (seq 1, 2), group b, then the ungrouped job last"
    );
    // The claim carries what the scheduler publishes with, and the version.
    assert_eq!(first[0].version, c.updated_at);
    assert_eq!(first[0].job_created_at, c.created_at);
    assert_eq!(first[0].mode, "IMMEDIATE");
    for row in &first {
        assert!(claimed_at(&pool, &row.job_id).await.is_some());
    }

    // Nothing is returned twice.
    assert!(lifecycle::claim(&pool, 100, &paused)
        .await
        .unwrap()
        .is_empty());

    // Released: it is claimable again, and only it.
    assert_eq!(
        lifecycle::release_claims(&pool, &[c.id.clone()])
            .await
            .unwrap(),
        1
    );
    assert_eq!(claimed_at(&pool, &c.id).await, None);
    assert_eq!(
        ids(&lifecycle::claim(&pool, 100, &paused).await.unwrap()),
        vec![c.id.clone()]
    );
    // Releasing what is not claimed, or not there, changes nothing.
    let ghost = "ghost0000000".to_string();
    assert_eq!(
        lifecycle::release_claims(&pool, &[e.id.clone(), ghost])
            .await
            .unwrap(),
        0
    );

    // Re-entering PENDING (a retry, a deferral, a hold) resets the claim.
    assert!(lifecycle::defer(
        &pool,
        &c.id,
        c.created_at,
        Utc::now() - Duration::seconds(1)
    )
    .await
    .unwrap());
    assert_eq!(claimed_at(&pool, &c.id).await, None);
    assert_eq!(
        ids(&lifecycle::claim(&pool, 100, &paused).await.unwrap()),
        vec![c.id.clone()]
    );
    // With the connection no longer paused, f is claimable too.
    assert_eq!(
        ids(&lifecycle::claim(&pool, 100, &[]).await.unwrap()),
        vec![f.id.clone()]
    );
}

/// The claim honours its limit and takes the head of the order.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_claim_takes_the_head_of_the_order_up_to_its_limit() {
    let (pool, _c) = setup_db().await;
    for n in 1..=10 {
        let mut s = spec(n);
        s.group = Some("g");
        s.sequence = (11 - n) as i32; // sequence descending in creation order
        put(&pool, &s).await;
    }
    let got = lifecycle::claim(&pool, 4, &[]).await.unwrap();
    let want: Vec<String> = (7..=10).rev().map(|n| spec(n).id).collect();
    assert_eq!(ids(&got), want, "lowest sequence first");
    let rest = lifecycle::claim(&pool, 100, &[]).await.unwrap();
    assert_eq!(rest.len(), 6);
    assert!(rest
        .windows(2)
        .all(|w| w[0].order_key() <= w[1].order_key()));
}

/// Concurrent claims never return the same row, and together take them all.
#[tokio::test]
#[ignore = "requires Docker"]
async fn concurrent_claims_never_return_the_same_row() {
    let (pool, _c) = setup_db().await;
    for n in 1..=300 {
        let mut s = spec(n);
        s.group = (n % 3 != 0).then_some("g");
        s.sequence = (n % 7) as i32;
        put(&pool, &s).await;
    }
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let pool = pool.clone();
        tasks.push(tokio::spawn(async move {
            let mut mine = Vec::new();
            loop {
                let got = lifecycle::claim(&pool, 25, &[]).await.unwrap();
                if got.is_empty() {
                    break;
                }
                assert!(got.windows(2).all(|w| w[0].order_key() <= w[1].order_key()));
                mine.extend(ids(&got));
            }
            mine
        }));
    }
    let mut all = Vec::new();
    for t in tasks {
        all.extend(t.await.unwrap());
    }
    let unique: HashSet<&String> = all.iter().collect();
    assert_eq!(all.len(), 300, "every row claimed");
    assert_eq!(unique.len(), 300, "and none twice");
}

// ── The hold-back ───────────────────────────────────────────────────────

/// At claim time a BLOCK_ON_ERROR job is held by an earlier FAILED / ERROR
/// job of its group, or by an earlier PENDING job in a retry backoff; the
/// holder itself is not held; IMMEDIATE jobs and other groups are free; a
/// job whose holder is QUEUED, or in the past, is free. The delivery-time
/// check (`group_held_before`) answers the same.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_blocked_group_is_held_at_claim_time_and_at_delivery_from_both_sources() {
    let (pool, _c) = setup_db().await;
    let hold = |n: i64, group: &'static str, seq: i32, status: &'static str| {
        let mut s = spec(n);
        s.group = Some(group);
        s.sequence = seq;
        s.status = status;
        s.mode = "BLOCK_ON_ERROR";
        s
    };
    // g1: a FAILED head; its BLOCK_ON_ERROR successors are held, an
    // IMMEDIATE one is not.
    let g1_failed = hold(1, "g1", 1, "FAILED");
    let g1_held = hold(2, "g1", 2, "PENDING");
    let mut g1_free = hold(3, "g1", 3, "PENDING");
    g1_free.mode = "IMMEDIATE";
    // g2: an ERROR head holds as well.
    let g2_error = hold(4, "g2", 1, "ERROR");
    let g2_held = hold(5, "g2", 2, "PENDING");
    // g3: a PENDING head in a retry backoff holds; it is itself not due.
    let mut g3_backoff = hold(6, "g3", 1, "PENDING");
    g3_backoff.scheduled_for = Some(Utc::now() + Duration::hours(1));
    let g3_held = hold(7, "g3", 2, "PENDING");
    // g4: a QUEUED head and a COMPLETED one hold nothing.
    let g4_queued = hold(8, "g4", 1, "QUEUED");
    let g4_free = hold(9, "g4", 2, "PENDING");
    // g5: a FAILED job LATER than the pending one does not hold it.
    let g5_first = hold(10, "g5", 1, "PENDING");
    let g5_failed = hold(11, "g5", 2, "FAILED");
    // Ungrouped BLOCK_ON_ERROR jobs are held only by a group named `default`.
    let mut loose = spec(12);
    loose.mode = "BLOCK_ON_ERROR";
    let default_failed = hold(13, "default", 0, "FAILED");
    for s in [
        &g1_failed,
        &g1_held,
        &g1_free,
        &g2_error,
        &g2_held,
        &g3_backoff,
        &g3_held,
        &g4_queued,
        &g4_free,
        &g5_first,
        &g5_failed,
        &loose,
        &default_failed,
    ] {
        put(&pool, s).await;
    }

    let got: HashSet<String> = ids(&lifecycle::claim(&pool, 100, &[]).await.unwrap())
        .into_iter()
        .collect();
    let want: HashSet<String> = [&g1_free, &g4_free, &g5_first]
        .iter()
        .map(|s| s.id.clone())
        .collect();
    assert_eq!(got, want, "claimed: only the jobs nothing holds");

    // The delivery-time check, for every PENDING or QUEUED BLOCK_ON_ERROR job.
    for (s, held) in [
        (&g1_held, true),
        (&g2_held, true),
        (&g3_held, true),
        (&g3_backoff, false), // the holder is not held by its own presence
        (&g4_queued, false),
        (&g4_free, false),
        (&g5_first, false),
    ] {
        let group = s.group.unwrap();
        assert_eq!(
            lifecycle::group_held_before(&pool, group, s.sequence, s.created_at, &s.id)
                .await
                .unwrap(),
            held,
            "{} in {group}",
            s.id
        );
    }
    // Once the backoff has expired the holder is due and holds nothing; the
    // successor is free.
    lifecycle::defer(
        &pool,
        &g3_backoff.id,
        g3_backoff.created_at,
        Utc::now() - Duration::seconds(1),
    )
    .await
    .unwrap();
    assert!(
        !lifecycle::group_held_before(&pool, "g3", 2, g3_held.created_at, &g3_held.id)
            .await
            .unwrap()
    );
    let got: HashSet<String> = ids(&lifecycle::claim(&pool, 100, &[]).await.unwrap())
        .into_iter()
        .collect();
    let want: HashSet<String> = [&g3_backoff, &g3_held]
        .iter()
        .map(|s| s.id.clone())
        .collect();
    assert_eq!(got, want, "the due holder and its successor");
}

// ── Stale claims, backlog ───────────────────────────────────────────────

/// The release of claims nobody holds: all of them (leader start), or only
/// the old ones (the periodic sweep); the caller's own are never touched.
#[tokio::test]
#[ignore = "requires Docker"]
async fn unheld_claims_are_released_but_never_the_callers() {
    let (pool, _c) = setup_db().await;
    let (old, young, held) = (spec(1), spec(2), spec(3));
    for s in [&old, &young, &held] {
        put(&pool, s).await;
    }
    for (id, ago) in [(&old.id, 10), (&young.id, 1), (&held.id, 10)] {
        sqlx::query("UPDATE msg_dispatch_queue SET claimed_at = $2 WHERE job_id = $1")
            .bind(id)
            .bind(Utc::now() - Duration::minutes(ago))
            .execute(&pool)
            .await
            .unwrap();
    }
    let mine = vec![held.id.clone()];
    let cutoff = Some(Utc::now() - Duration::minutes(5));
    assert_eq!(
        lifecycle::release_unheld_claims(&pool, cutoff, &mine)
            .await
            .unwrap(),
        1,
        "only the old, unheld one"
    );
    assert_eq!(claimed_at(&pool, &old.id).await, None);
    assert!(claimed_at(&pool, &young.id).await.is_some());
    assert!(claimed_at(&pool, &held.id).await.is_some());

    assert_eq!(
        lifecycle::release_unheld_claims(&pool, None, &mine)
            .await
            .unwrap(),
        1,
        "at leader start every claim it does not hold"
    );
    assert_eq!(claimed_at(&pool, &young.id).await, None);
    assert!(claimed_at(&pool, &held.id).await.is_some());
}

/// The backlog counts unclaimed, due rows and reports the oldest.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_backlog_is_the_unclaimed_due_rows() {
    let (pool, _c) = setup_db().await;
    let empty = lifecycle::queue_backlog(&pool).await.unwrap();
    assert_eq!((empty.depth, empty.oldest_enqueued_at), (0, None));
    let (a, b, c, d) = (spec(1), spec(2), spec(3), spec(4));
    let mut later = d.clone();
    later.scheduled_for = Some(Utc::now() + Duration::hours(1));
    for s in [&a, &b, &c, &later] {
        put(&pool, s).await;
    }
    sqlx::query(
        "UPDATE msg_dispatch_queue SET enqueued_at = NOW() - INTERVAL '10 minutes' WHERE job_id = $1",
    )
    .bind(&b.id)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE msg_dispatch_queue SET claimed_at = NOW() WHERE job_id = $1")
        .bind(&c.id)
        .execute(&pool)
        .await
        .unwrap();
    let backlog = lifecycle::queue_backlog(&pool).await.unwrap();
    assert_eq!(backlog.depth, 2, "a and b: c is claimed, d is not due");
    let age = Utc::now() - backlog.oldest_enqueued_at.unwrap();
    assert!(age >= Duration::minutes(10) && age < Duration::minutes(11));
}

// ── The reconcile sweep ─────────────────────────────────────────────────

async fn queue_snapshot(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT (job_id, job_created_at, message_group, sequence, scheduled_for, mode, version, \
                 claimed_at)::text FROM msg_dispatch_queue ORDER BY job_id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

/// Each of (a) insert, (b) delete, (c) refresh repairs a hand-made
/// corruption and reports it; a consistent table reports zero and changes
/// nothing; a just-created job and a young claim are left alone.
#[tokio::test]
#[ignore = "requires Docker"]
async fn reconcile_repairs_each_kind_of_drift_and_leaves_a_consistent_table_alone() {
    let (pool, _c) = setup_db().await;
    let (p1, p2, p3) = (spec(1), spec(2), spec(3));
    let mut done_unclaimed = spec(4);
    done_unclaimed.status = "COMPLETED";
    let mut done_old_claim = spec(5);
    done_old_claim.status = "COMPLETED";
    let mut done_young_claim = spec(6);
    done_young_claim.status = "COMPLETED";
    let mut young = spec(7);
    young.updated_at = Utc::now(); // created a moment ago
    for s in [
        &p1,
        &p2,
        &p3,
        &done_unclaimed,
        &done_old_claim,
        &done_young_claim,
    ] {
        put(&pool, s).await;
    }
    // `young` is a PENDING job whose queue row was never written.
    put(&pool, &young).await;
    sqlx::query("DELETE FROM msg_dispatch_queue WHERE job_id = $1")
        .bind(&young.id)
        .execute(&pool)
        .await
        .unwrap();

    // A consistent table: nothing to report, nothing changes.
    let before = queue_snapshot(&pool).await;
    let none = lifecycle::reconcile_queue(&pool, ReconcileGuards::production())
        .await
        .unwrap();
    assert_eq!(none, Reconciled::default(), "{none:?}");
    assert_eq!(queue_snapshot(&pool).await, before);

    // (a) a PENDING job without a queue row.
    sqlx::query("DELETE FROM msg_dispatch_queue WHERE job_id = $1")
        .bind(&p1.id)
        .execute(&pool)
        .await
        .unwrap();
    // (c) a row whose version is not the job's.
    sqlx::query(
        "UPDATE msg_dispatch_queue SET version = version - INTERVAL '1 second', \
         sequence = 77, claimed_at = NOW() - INTERVAL '10 minutes' WHERE job_id = $1",
    )
    .bind(&p2.id)
    .execute(&pool)
    .await
    .unwrap();
    // (b) rows whose job is gone, not PENDING, or does not exist.
    for (id, created_at, claimed) in [
        (&done_unclaimed.id, done_unclaimed.created_at, None),
        (
            &done_old_claim.id,
            done_old_claim.created_at,
            Some(Utc::now() - Duration::minutes(10)),
        ),
        (
            &done_young_claim.id,
            done_young_claim.created_at,
            Some(Utc::now() - Duration::minutes(1)),
        ),
        (&"ghost0000000".to_string(), Utc::now(), None),
    ] {
        sqlx::query(
            "INSERT INTO msg_dispatch_queue (job_id, job_created_at, sequence, mode, version, \
             claimed_at) VALUES ($1, $2, 1, 'IMMEDIATE', NOW(), $3)",
        )
        .bind(id)
        .bind(created_at)
        .bind(claimed)
        .execute(&pool)
        .await
        .unwrap();
    }

    // Production guards: `young` (updated a moment ago) and the young claim
    // are not drift yet.
    let done = lifecycle::reconcile_queue(&pool, ReconcileGuards::production())
        .await
        .unwrap();
    assert_eq!(
        done,
        Reconciled {
            inserted: 1,  // p1
            deleted: 3,   // done_unclaimed, done_old_claim, ghost
            refreshed: 1, // p2
        }
    );
    let rows = queue_ids(&pool).await;
    assert!(rows.contains(&p1.id), "(a) p1 is back");
    assert!(!rows.contains(&"ghost0000000".to_string()), "(b)");
    assert!(!rows.contains(&done_unclaimed.id), "(b)");
    assert!(!rows.contains(&done_old_claim.id), "(b)");
    assert!(rows.contains(&done_young_claim.id), "a young claim is left");
    assert!(!rows.contains(&young.id), "a young job is left alone");
    let (seq, claimed): (i32, Option<DateTime<Utc>>) =
        sqlx::query_as("SELECT sequence, claimed_at FROM msg_dispatch_queue WHERE job_id = $1")
            .bind(&p2.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((seq, claimed), (1, None), "(c) p2 mirrors its job again");

    // A second pass has nothing left but the two things the guards protect.
    let again = lifecycle::reconcile_queue(&pool, ReconcileGuards::production())
        .await
        .unwrap();
    assert_eq!(again, Reconciled::default());
    let drift = lifecycle::queue_drift(&pool).await.unwrap();
    assert_eq!(
        drift,
        lifecycle::QueueDrift {
            missing_or_stale: 1, // young
            orphaned: 1,         // done_young_claim
        }
    );

    // Without the guards they go too, and the table is exact.
    let rest = lifecycle::reconcile_queue(&pool, ReconcileGuards::immediate())
        .await
        .unwrap();
    assert_eq!(
        rest,
        Reconciled {
            inserted: 1,
            deleted: 1,
            refreshed: 0
        }
    );
    assert_eq!(
        lifecycle::queue_drift(&pool).await.unwrap(),
        lifecycle::QueueDrift {
            missing_or_stale: 0,
            orphaned: 0
        }
    );
}

/// The sweep is bounded: it repairs at most `limit` rows of each kind.
#[tokio::test]
#[ignore = "requires Docker"]
async fn reconcile_is_bounded_per_pass() {
    let (pool, _c) = setup_db().await;
    for n in 1..=20 {
        put(&pool, &spec(n)).await;
    }
    sqlx::query("DELETE FROM msg_dispatch_queue")
        .execute(&pool)
        .await
        .unwrap();
    let mut guards = ReconcileGuards::immediate();
    guards.limit = 8;
    let first = lifecycle::reconcile_queue(&pool, guards).await.unwrap();
    assert_eq!(first.inserted, 8);
    let second = lifecycle::reconcile_queue(&pool, guards).await.unwrap();
    assert_eq!(second.inserted, 8);
    let third = lifecycle::reconcile_queue(&pool, guards).await.unwrap();
    assert_eq!(third.inserted, 4);
    assert_eq!(queue_ids(&pool).await.len(), 20);
}

// ── Migration 065 ───────────────────────────────────────────────────────

/// The index set after 065: the three partial indexes the dispatch path read
/// through are gone, one ordinary index replaces them, the queue table has
/// its storage options, and the only partial index left on a dispatch table
/// is the projector's.
#[tokio::test]
#[ignore = "requires Docker"]
async fn migration_065_swaps_the_indexes_and_sets_the_queue_storage_options() {
    let (pool, _c) = setup_db().await;
    let indexes: Vec<(String, String)> = sqlx::query_as(
        "SELECT indexname::text, indexdef::text FROM pg_indexes \
         WHERE schemaname = 'public' AND tablename IN ('msg_dispatch_jobs', 'msg_dispatch_queue')",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let names: HashSet<&str> = indexes.iter().map(|(n, _)| n.as_str()).collect();
    for gone in [
        "idx_dispatch_jobs_pending_poll",
        "idx_dispatch_jobs_group_holders",
        "idx_dispatch_jobs_in_flight",
    ] {
        assert!(!names.contains(gone), "{gone} must be dropped");
    }
    let (_, def) = indexes
        .iter()
        .find(|(n, _)| n == "idx_dispatch_jobs_status_group")
        .expect("the new index");
    assert!(
        def.contains("(status, message_group, sequence, created_at, id)")
            && !def.contains(" WHERE "),
        "{def}"
    );
    // The partial indexes left on the two tables (a partitioned table's
    // `pg_indexes` rows are its own; the partitions' copies are listed under
    // the partitions' names).
    let partial: HashSet<&str> = indexes
        .iter()
        .filter(|(_, def)| def.contains(" WHERE "))
        .map(|(n, _)| n.as_str())
        .collect();
    assert_eq!(
        partial,
        HashSet::from(["idx_msg_dispatch_jobs_dirty"]),
        "the projector's is the one partial index left"
    );
    // Every partial index of the dispatch tables, for the record.
    let all_partial: Vec<(String, String)> = sqlx::query_as(
        "SELECT tablename::text, indexname::text FROM pg_indexes \
         WHERE schemaname = 'public' AND tablename ~ '^msg_dispatch_[a-z_]+$' \
           AND tablename !~ '_[0-9]{4}_[0-9]{2}$' AND indexdef LIKE '% WHERE %' ORDER BY 1, 2",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    println!("partial indexes on dispatch tables: {all_partial:?}");

    let options: Vec<String> = sqlx::query_scalar(
        "SELECT unnest(reloptions) FROM pg_class WHERE relname = 'msg_dispatch_queue'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    for want in [
        "fillfactor=70",
        "autovacuum_vacuum_scale_factor=0",
        "autovacuum_vacuum_threshold=2000",
        "autovacuum_analyze_scale_factor=0",
        "autovacuum_analyze_threshold=2000",
    ] {
        assert!(options.contains(&want.to_string()), "{want} in {options:?}");
    }
    // `claimed_at` is in no index.
    let claimed_indexed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pg_indexes WHERE tablename = 'msg_dispatch_queue' \
         AND indexdef LIKE '%claimed_at%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(claimed_indexed, 0);
}

/// The migration repairs what an older binary left (a missing row, an
/// orphan), is idempotent, and is recorded without re-running when another
/// platform's copy of it already ran (the probe finds the new index).
#[tokio::test]
#[ignore = "requires Docker"]
async fn migration_065_repairs_drift_and_is_recognised_when_already_applied() {
    let (pool, _c) = setup_db().await;
    let (p1, p2) = (spec(1), spec(2));
    let mut done = spec(3);
    done.status = "COMPLETED";
    for s in [&p1, &p2, &done] {
        put(&pool, s).await;
    }
    // An older binary: p1 has no queue row, `done` has a stale one.
    sqlx::query("DELETE FROM msg_dispatch_queue WHERE job_id = $1")
        .bind(&p1.id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO msg_dispatch_queue (job_id, job_created_at, sequence, mode, version) \
         VALUES ($1, $2, 1, 'IMMEDIATE', NOW())",
    )
    .bind(&done.id)
    .bind(done.created_at)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query("DELETE FROM _schema_migrations WHERE migration_id = '065_dispatch_queue_reads'")
        .execute(&pool)
        .await
        .unwrap();
    run_migrations(&pool, MigrationProfile::Production)
        .await
        .unwrap();
    let mut want = vec![p1.id.clone(), p2.id.clone()];
    want.sort();
    assert_eq!(queue_ids(&pool).await, want, "repaired by the migration");

    // Re-running its SQL changes nothing.
    let before = queue_snapshot(&pool).await;
    sqlx::raw_sql(include_str!(
        "../../../../migrations/065_dispatch_queue_reads.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(queue_snapshot(&pool).await, before);

    // A tracker that predates it (a database another platform migrated): the
    // probe records it and runs nothing.
    sqlx::query("DELETE FROM msg_dispatch_queue WHERE job_id = $1")
        .bind(&p1.id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM _schema_migrations")
        .execute(&pool)
        .await
        .unwrap();
    run_migrations(&pool, MigrationProfile::Production)
        .await
        .unwrap();
    let tracked: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM _schema_migrations \
         WHERE migration_id = '065_dispatch_queue_reads')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(tracked);
    assert!(
        !queue_ids(&pool).await.contains(&p1.id),
        "the probe did not run the migration's repair"
    );
}
