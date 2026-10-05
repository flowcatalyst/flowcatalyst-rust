//! The scheduler's claim and hold-back as reads of the job table, against a
//! real PostgreSQL: the claim (order, what it skips), the in-flight
//! exclusion, the hold-back (claim time and delivery time), the bounded
//! backlog sample, migration 066 and the scheduler's pool settings. Requires
//! Docker, or a local PostgreSQL through `FC_TEST_PG_BIN` (see
//! `support/db.rs`):
//!   cargo test -p fc-platform --test it dispatch_claim_test:: -- --ignored

use std::collections::HashSet;

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;

use crate::support::{start_db, TestDb};
use fc_platform::dispatch_job::lifecycle;
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
}

fn ids(rows: &[lifecycle::ClaimRow]) -> Vec<String> {
    rows.iter().map(|r| r.job_id.clone()).collect()
}

async fn claim(
    pool: &PgPool,
    limit: i64,
    paused: &[String],
    held: &[String],
    in_flight: &[String],
) -> Vec<lifecycle::ClaimRow> {
    lifecycle::claim(pool, limit, paused, held, in_flight)
        .await
        .unwrap()
}

// ── The claim ───────────────────────────────────────────────────────────

/// The claim returns PENDING jobs in the total order (group, NULLS LAST;
/// sequence; created_at; id); skips not-yet-due jobs, paused subscriptions,
/// remembered held groups and the in-flight ids; writes nothing.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_claim_reads_pending_jobs_in_order_and_skips_what_it_is_told_to() {
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
    let mut h = spec(7);
    h.group = Some("held");
    let mut done = spec(8);
    done.status = "QUEUED"; // not PENDING
    let mut flying = spec(9);
    flying.group = Some("z");
    for s in [&a, &b, &c, &d, &e, &f, &h, &done, &flying] {
        put(&pool, s).await;
    }
    let before: Vec<(String, DateTime<Utc>)> =
        sqlx::query_as("SELECT id, updated_at FROM msg_dispatch_jobs ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();

    let paused = vec!["sub_paused".to_string()];
    let skip = vec!["held".to_string()];
    let flying_ids = vec![flying.id.clone()];
    let got = claim(&pool, 100, &paused, &skip, &flying_ids).await;
    assert_eq!(
        ids(&got),
        vec![c.id.clone(), b.id.clone(), a.id.clone(), d.id.clone()],
        "group a (seq 1, 2), group b, then the ungrouped job last"
    );
    // What the scheduler publishes with, and the version it marks against.
    assert_eq!(got[0].version, c.updated_at);
    assert_eq!(got[0].job_created_at, c.created_at);
    assert_eq!(got[0].mode, "IMMEDIATE");
    // A read: nothing changed, and the same claim returns the same rows.
    let after: Vec<(String, DateTime<Utc>)> =
        sqlx::query_as("SELECT id, updated_at FROM msg_dispatch_jobs ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(before, after);
    assert_eq!(claim(&pool, 100, &paused, &skip, &flying_ids).await, got);
    // Without the exclusions the skipped ones are there.
    let all = claim(&pool, 100, &[], &[], &[]).await;
    let all_ids: HashSet<String> = ids(&all).into_iter().collect();
    for want in [&f, &h, &flying] {
        assert!(all_ids.contains(&want.id), "{}", want.id);
    }
    assert!(!all_ids.contains(&e.id) && !all_ids.contains(&done.id));
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
    let got = claim(&pool, 4, &[], &[], &[]).await;
    let want: Vec<String> = (7..=10).rev().map(|n| spec(n).id).collect();
    assert_eq!(ids(&got), want, "lowest sequence first");
    // With the first four in flight the next claim is the next four.
    let next = claim(&pool, 4, &[], &[], &ids(&got)).await;
    let want: Vec<String> = (3..=6).rev().map(|n| spec(n).id).collect();
    assert_eq!(ids(&next), want);
}

// ── The hold-back ───────────────────────────────────────────────────────

/// A claimed BLOCK_ON_ERROR job is held by an earlier FAILED / ERROR job of
/// its group, or by an earlier PENDING job in a retry backoff (not due, so
/// not claimed); the holder itself is not held; IMMEDIATE jobs and other
/// groups are free; a job whose holder is QUEUED, or due, is free. The
/// delivery-time check (`group_held_before`) answers the same. The probe for
/// backoff holders is bounded by the group's last candidate, so a holder
/// positioned AFTER every candidate does not hold them.
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
    let g1_failed = hold(1, "g1", 1, "FAILED");
    let g1_held = hold(2, "g1", 2, "PENDING");
    let mut g1_free = hold(3, "g1", 3, "PENDING");
    g1_free.mode = "IMMEDIATE";
    let g2_error = hold(4, "g2", 1, "ERROR");
    let g2_held = hold(5, "g2", 2, "PENDING");
    let mut g3_backoff = hold(6, "g3", 1, "PENDING");
    g3_backoff.scheduled_for = Some(Utc::now() + Duration::hours(1));
    let g3_held = hold(7, "g3", 2, "PENDING");
    let g4_queued = hold(8, "g4", 1, "QUEUED");
    let g4_free = hold(9, "g4", 2, "PENDING");
    let g5_first = hold(10, "g5", 1, "PENDING");
    let g5_failed = hold(11, "g5", 2, "FAILED");
    // A backoff holder AFTER the candidate holds nothing.
    let g6_first = hold(12, "g6", 1, "PENDING");
    let mut g6_later = hold(13, "g6", 2, "PENDING");
    g6_later.scheduled_for = Some(Utc::now() + Duration::hours(1));
    // Ungrouped BLOCK_ON_ERROR jobs are held only by a group named `default`.
    let mut loose = spec(14);
    loose.mode = "BLOCK_ON_ERROR";
    let default_failed = hold(15, "default", 0, "FAILED");
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
        &g6_first,
        &g6_later,
        &loose,
        &default_failed,
    ] {
        put(&pool, s).await;
    }

    let claimed = claim(&pool, 100, &[], &[], &[]).await;
    assert!(
        !ids(&claimed).contains(&g3_backoff.id),
        "not due: not claimed"
    );
    let got: HashSet<String> = lifecycle::held_among(&pool, &claimed)
        .await
        .unwrap()
        .into_iter()
        .collect();
    let want: HashSet<String> = [&g1_held, &g2_held, &g3_held, &loose]
        .iter()
        .map(|s| s.id.clone())
        .collect();
    assert_eq!(got, want, "held: behind a FAILED/ERROR/backoff holder");
    assert!(lifecycle::held_among(&pool, &[]).await.unwrap().is_empty());

    // The delivery-time check.
    for (s, held) in [
        (&g1_held, true),
        (&g2_held, true),
        (&g3_held, true),
        (&g3_backoff, false), // the holder is not held by its own presence
        (&g4_queued, false),
        (&g4_free, false),
        (&g5_first, false),
        (&g6_first, false),
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
}

// ── The backlog ─────────────────────────────────────────────────────────

/// The backlog sample counts PENDING jobs and reports the first in claim
/// order; it saturates at 100,000 instead of scanning a bigger backlog.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_backlog_sample_is_bounded() {
    let (pool, _c) = setup_db().await;
    let empty = lifecycle::pending_backlog(&pool).await.unwrap();
    assert_eq!(
        (empty.depth, empty.saturated, empty.oldest_created_at),
        (0, false, None)
    );
    let mut a = spec(1);
    a.group = Some("b");
    let mut b = spec(2);
    b.group = Some("a");
    let mut done = spec(3);
    done.status = "COMPLETED";
    for s in [&a, &b, &done] {
        put(&pool, s).await;
    }
    let small = lifecycle::pending_backlog(&pool).await.unwrap();
    assert_eq!((small.depth, small.saturated), (2, false));
    assert_eq!(
        small.oldest_created_at,
        Some(b.created_at),
        "group a is first in claim order"
    );

    sqlx::query(
        "INSERT INTO msg_dispatch_jobs (id, code, target_url, status, created_at, updated_at) \
         SELECT 'B' || lpad(n::text, 12, '0'), 'a:b:c:d', 'http://x/h', 'PENDING', now(), now() \
           FROM generate_series(1, 100010) n",
    )
    .execute(&pool)
    .await
    .unwrap();
    let big = lifecycle::pending_backlog(&pool).await.unwrap();
    assert_eq!((big.depth, big.saturated), (100_000, true));
}

// ── Migration 066 and the pool ──────────────────────────────────────────

/// The queue table is gone, the plain status index stays, the three partial
/// indexes stay dropped, the projector's is the one partial index left on
/// `msg_dispatch_jobs`, and the migration is idempotent and recognised when a
/// tracker predates it (a database another platform migrated).
#[tokio::test]
#[ignore = "requires Docker"]
async fn migration_066_retires_the_queue_table_and_keeps_the_plain_index() {
    let (pool, _c) = setup_db().await;
    let table: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_name = 'msg_dispatch_queue')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!table, "msg_dispatch_queue must be dropped");

    let indexes: Vec<(String, String)> = sqlx::query_as(
        "SELECT indexname::text, indexdef::text FROM pg_indexes \
         WHERE schemaname = 'public' AND tablename = 'msg_dispatch_jobs'",
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
        assert!(!names.contains(gone), "{gone} must stay dropped");
    }
    let (_, def) = indexes
        .iter()
        .find(|(n, _)| n == "idx_dispatch_jobs_status_group")
        .expect("the plain status index stays");
    assert!(
        def.contains("(status, message_group, sequence, created_at, id)")
            && !def.contains(" WHERE "),
        "{def}"
    );
    let partial: HashSet<&str> = indexes
        .iter()
        .filter(|(_, def)| def.contains(" WHERE "))
        .map(|(n, _)| n.as_str())
        .collect();
    assert_eq!(partial, HashSet::from(["idx_msg_dispatch_jobs_dirty"]));

    // Idempotent.
    sqlx::raw_sql(include_str!(
        "../../../../migrations/066_drop_dispatch_queue.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    // A tracker that predates it: the probes record 064-066 and run nothing
    // (in particular 064 must not recreate the table).
    sqlx::query("DELETE FROM _schema_migrations")
        .execute(&pool)
        .await
        .unwrap();
    run_migrations(&pool, MigrationProfile::Production)
        .await
        .unwrap();
    let table: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_name = 'msg_dispatch_queue')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        !table,
        "adopting a migrated database must not recreate the queue table"
    );
    let tracked: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM _schema_migrations WHERE migration_id IN \
         ('064_dispatch_queue', '065_dispatch_queue_reads', '066_drop_dispatch_queue')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tracked, 3);
}

/// Every connection of the scheduler's own pool runs with
/// `plan_cache_mode = force_custom_plan` and `enable_sort = off`; the
/// platform's pool keeps the server's defaults.
#[tokio::test]
#[ignore = "requires Docker"]
async fn only_the_schedulers_pool_sets_the_planner_options() {
    use fc_platform::shared::database::create_scheduler_pool;
    let (_c, url) = start_db("fc").await;
    let scheduler = create_scheduler_pool(&url, 3).await.unwrap();
    let platform = create_pool(&url).await.unwrap();
    for _ in 0..6 {
        let (a, b): (String, String) = sqlx::query_as(
            "SELECT current_setting('plan_cache_mode'), current_setting('enable_sort')",
        )
        .fetch_one(&scheduler)
        .await
        .unwrap();
        assert_eq!((a.as_str(), b.as_str()), ("force_custom_plan", "off"));
    }
    let (a, b): (String, String) =
        sqlx::query_as("SELECT current_setting('plan_cache_mode'), current_setting('enable_sort')")
            .fetch_one(&platform)
            .await
            .unwrap();
    assert_eq!((a.as_str(), b.as_str()), ("auto", "on"));
}
