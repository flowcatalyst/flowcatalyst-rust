//! Query plans of the dispatch queue's reads, against a real PostgreSQL with
//! a seeded table (about 910,000 rows, mostly COMPLETED, 5,000 QUEUED, 5,000 PENDING, groups
//! with FAILED holders, several populated monthly partitions), in the three
//! statistics states a production table can be in:
//!
//! * `analysed`: ANALYZE after the load;
//! * `analysed_while_empty`: ANALYZE ran on the empty tables, then the load
//!   (autovacuum off, so the statistics stay the empty ones);
//! * `never_analysed`: no statistics at all.
//!
//! For each statement the plan must use the intended index with bind
//! parameters (both as a custom plan and as a GENERIC plan, which is what a
//! cached prepared statement runs after a few executions), must not sort the
//! claim, must stop the claim's index walk early, and must not seq-scan
//! `msg_dispatch_jobs`. `-- --nocapture` prints every plan summary and its
//! timing. Requires Docker, or a local PostgreSQL (`FC_TEST_PG_BIN`).

use std::collections::BTreeSet;
use std::time::Instant;

use serde_json::Value;
use sqlx::postgres::PgArguments;
use sqlx::query::QueryScalar;
use sqlx::PgPool;
use sqlx::Postgres;

use crate::support::{start_db, TestDb};
use fc_platform::dispatch_job::lifecycle;
use fc_platform::shared::database::{create_pool, run_migrations, MigrationProfile};

/// Rows by status in the seeded table.
const COMPLETED: i64 = 900_000;
/// QUEUED rows in the plan tests (a busy table); the sweeps are also timed at
/// 100,000.
const QUEUED: i64 = 5_000;
const QUEUED_AT_SCALE: i64 = 100_000;
const PENDING: i64 = 5_000;
/// A deep queue, for the claim's plan with no statistics.
const PENDING_DEEP: i64 = 150_000;

/// FAILED holders: 100 of the 500 groups are held.
const FAILED: i64 = 100;
const PROCESSING: i64 = 300;

#[derive(Clone, Copy, Debug)]
enum Stats {
    Analysed,
    AnalysedWhileEmpty,
    NeverAnalysed,
}

async fn exec(pool: &PgPool, sql: &str) {
    sqlx::raw_sql(sql)
        .execute(pool)
        .await
        .unwrap_or_else(|e| panic!("{e}: {sql}"));
}

/// Months of partitions the table has: the migration creates last month
/// through twelve ahead; the load spreads over four of them.
async fn seed(pool: &PgPool, stats: Stats, queued: i64, pending: i64) -> TestDbSizes {
    // No autovacuum: the statistics stay what the test made them.
    exec(
        pool,
        "DO $$ DECLARE r record; BEGIN \
           FOR r IN SELECT inhrelid::regclass AS t FROM pg_inherits \
                     WHERE inhparent = 'msg_dispatch_jobs'::regclass LOOP \
             EXECUTE format('ALTER TABLE %s SET (autovacuum_enabled = false)', r.t); \
           END LOOP; \
           ALTER TABLE msg_dispatch_queue SET (autovacuum_enabled = false); \
         END $$",
    )
    .await;
    if matches!(stats, Stats::AnalysedWhileEmpty) {
        exec(
            pool,
            "ANALYZE msg_dispatch_jobs; ANALYZE msg_dispatch_queue",
        )
        .await;
    }

    // created_at: spread over the last month and the next two (four
    // populated partitions).
    let created = "date_trunc('month', now()) - interval '25 days' \
                   + (random() * interval '95 days')";
    // One statement per status; ids are unique across them.
    let load = |from: i64, count: i64, status: &str, extra: &str| {
        format!(
            "INSERT INTO msg_dispatch_jobs (id, code, source, target_url, payload, status, mode, \
                 message_group, sequence, created_at, updated_at, subscription_id, scheduled_for) \
             SELECT 'J' || lpad(n::text, 12, '0'), 'a:b:c:d', 's', 'http://x/h', '{{}}', '{status}', \
                    {extra} \
               FROM (SELECT n, {created} AS c FROM generate_series({from}, {from} + {count} - 1) n) s"
        )
    };
    // COMPLETED / QUEUED: grouped and ungrouped mixed.
    exec(
        pool,
        &load(
            1,
            COMPLETED,
            "COMPLETED",
            "'IMMEDIATE', CASE WHEN n % 3 = 0 THEN NULL ELSE 'g' || lpad((n % 500)::text, 4, '0') END, \
             (n % 40)::int, c, c, NULL, NULL",
        ),
    )
    .await;
    exec(
        pool,
        &load(
            1_000_001,
            queued,
            "QUEUED",
            // Fresh, but 200 of them stale.
            &format!(
                "'BLOCK_ON_ERROR', 'g' || lpad((n % 500)::text, 4, '0'), (n % 40)::int, c, \
                 CASE WHEN n % {} = 0 THEN now() - interval '40 minutes' \
                      ELSE now() - random() * interval '10 minutes' END, NULL, NULL",
                queued / 200
            ),
        ),
    )
    .await;
    // PENDING: BLOCK_ON_ERROR in groups g0000..g0499 (the groups some FAILED
    // jobs hold), a share ungrouped IMMEDIATE, 1% in a backoff.
    exec(
        pool,
        &load(
            2_000_001,
            pending,
            "PENDING",
            "CASE WHEN n % 5 = 0 THEN 'IMMEDIATE' ELSE 'BLOCK_ON_ERROR' END, \
             CASE WHEN n % 5 = 0 THEN NULL ELSE 'g' || lpad((n % 500)::text, 4, '0') END, \
             (n % 40)::int, c, c, CASE WHEN n % 7 = 0 THEN 'sub_x' END, \
             CASE WHEN n % 100 = 0 THEN now() + interval '1 hour' END",
        ),
    )
    .await;
    // FAILED / ERROR holders in 400 of the groups; PROCESSING.
    exec(
        pool,
        &load(
            3_000_001,
            FAILED,
            "FAILED",
            "'BLOCK_ON_ERROR', 'g' || lpad((n % 500)::text, 4, '0'), (n % 40)::int, c, c, NULL, NULL",
        ),
    )
    .await;
    exec(
        pool,
        &load(
            4_000_001,
            PROCESSING,
            "PROCESSING",
            "'IMMEDIATE', NULL, 0, c, \
             CASE WHEN n % 15 = 0 THEN now() - interval '90 minutes' ELSE now() END, NULL, NULL",
        ),
    )
    .await;
    // The queue rows of the PENDING jobs (what the lifecycle would hold).
    exec(
        pool,
        "INSERT INTO msg_dispatch_queue (job_id, job_created_at, message_group, sequence, \
                scheduled_for, subscription_id, dispatch_pool_id, client_id, mode, queue, version) \
         SELECT id, created_at, message_group, sequence, scheduled_for, subscription_id, \
                dispatch_pool_id, client_id, mode, queue, updated_at \
           FROM msg_dispatch_jobs WHERE status = 'PENDING'",
    )
    .await;
    // A realistic head of the claim: 800 claimed rows (in flight) at the
    // front of the order.
    exec(
        pool,
        "UPDATE msg_dispatch_queue SET claimed_at = now() WHERE job_id IN ( \
           SELECT job_id FROM msg_dispatch_queue ORDER BY message_group NULLS LAST, sequence, \
                  job_created_at, job_id LIMIT 800)",
    )
    .await;
    if matches!(stats, Stats::Analysed) {
        exec(
            pool,
            "ANALYZE msg_dispatch_jobs; ANALYZE msg_dispatch_queue",
        )
        .await;
    }
    TestDbSizes {
        pending,
        claimed_head: 800,
    }
}

struct TestDbSizes {
    pending: i64,
    claimed_head: i64,
}

// ─── Plan inspection ────────────────────────────────────────────────────────

/// Every node of a plan tree.
fn nodes(plan: &Value) -> Vec<&Value> {
    let mut out = Vec::new();
    fn walk<'a>(n: &'a Value, out: &mut Vec<&'a Value>) {
        out.push(n);
        if let Some(children) = n.get("Plans").and_then(Value::as_array) {
            for c in children {
                walk(c, out);
            }
        }
    }
    let root = if plan.is_array() { &plan[0] } else { plan };
    walk(&root["Plan"], &mut out);
    out
}

fn text(n: &Value, key: &str) -> String {
    n.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// The names of the indexes the plan scans.
fn indexes(plan: &Value) -> BTreeSet<String> {
    nodes(plan)
        .iter()
        .map(|n| text(n, "Index Name"))
        .filter(|i| !i.is_empty())
        .collect()
}

/// Relations the plan seq-scans (a scan of an empty partition reads no block
/// and is not counted: the planner is right to prefer it).
fn seq_scans(plan: &Value) -> Vec<String> {
    nodes(plan)
        .iter()
        .filter(|n| text(n, "Node Type") == "Seq Scan")
        .filter(|n| {
            let blocks = n["Shared Hit Blocks"].as_f64().unwrap_or(0.0)
                + n["Shared Read Blocks"].as_f64().unwrap_or(0.0);
            // Without ANALYZE there is no block count: count the scan.
            n.get("Shared Hit Blocks").is_none() || blocks > 0.0
        })
        .map(|n| text(n, "Relation Name"))
        .collect()
}

fn node_types(plan: &Value) -> BTreeSet<String> {
    nodes(plan).iter().map(|n| text(n, "Node Type")).collect()
}

/// Every node of the plan with its row counts, one per line.
fn compact(plan: &Value) -> String {
    let mut out = String::from("\n");
    fn walk(n: &Value, depth: usize, out: &mut String) {
        let num = |k: &str| n.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        out.push_str(&format!(
            "{}{} {}{} rows={} loops={} removed={} time={:.1}ms\n",
            "  ".repeat(depth),
            text(n, "Node Type"),
            text(n, "Relation Name"),
            match text(n, "Index Name") {
                i if i.is_empty() => String::new(),
                i => format!(" [{i}]"),
            },
            num("Actual Rows"),
            num("Actual Loops"),
            num("Rows Removed by Filter"),
            num("Actual Total Time"),
        ));
        if let Some(children) = n.get("Plans").and_then(Value::as_array) {
            for c in children {
                walk(c, depth + 1, out);
            }
        }
    }
    let root = if plan.is_array() { &plan[0] } else { plan };
    walk(&root["Plan"], 0, &mut out);
    out
}

/// One line per plan, for `--nocapture`.
fn summary(name: &str, plan: &Value) -> String {
    let root = if plan.is_array() { &plan[0] } else { plan };
    let exec = root
        .get("Execution Time")
        .and_then(Value::as_f64)
        .map_or(String::new(), |t| format!(" exec={t:.1}ms"));
    format!(
        "{name}: nodes={:?} indexes={:?} seqscans={:?}{exec}",
        node_types(plan),
        indexes(plan),
        seq_scans(plan)
    )
}

/// The plan with every partition's copy of an index named as the index
/// of the partitioned table (`msg_dispatch_jobs_2026_09_status_..._idx`
/// becomes `idx_dispatch_jobs_status_group`).
async fn canon(pool: &PgPool, plan: Value) -> Value {
    let pairs: Vec<(String, String)> = sqlx::query_as(
        "SELECT c.relname::text, p.relname::text FROM pg_inherits i \
           JOIN pg_class c ON c.oid = i.inhrelid JOIN pg_class p ON p.oid = i.inhparent \
          WHERE p.relkind = 'I'",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    let mut json = plan.to_string();
    for (child, parent) in pairs {
        json = json.replace(&format!("\"{child}\""), &format!("\"{parent}\""));
    }
    serde_json::from_str(&json).unwrap()
}

async fn explain_custom(pool: &PgPool, sql: &str, analyze: bool, binds: Binds) -> Value {
    let opts = if analyze {
        "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) "
    } else {
        "EXPLAIN (FORMAT JSON) "
    };
    let mut tx = pool.begin().await.unwrap();
    let text = format!("{opts}{sql}");
    let q = binds.apply(sqlx::query_scalar::<_, Value>(&text));
    let plan = q.fetch_one(&mut *tx).await.unwrap();
    tx.rollback().await.unwrap();
    canon(pool, plan).await
}

/// The same statement as a prepared statement planned GENERICALLY (no
/// parameter values), executed as `EXPLAIN EXECUTE`.
async fn explain_generic(
    pool: &PgPool,
    name: &str,
    sql: &str,
    types: &str,
    args: &str,
    analyze: bool,
) -> Value {
    let opts = if analyze {
        "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON)"
    } else {
        "EXPLAIN (FORMAT JSON)"
    };
    let mut tx = pool.begin().await.unwrap();
    sqlx::raw_sql("SET LOCAL plan_cache_mode = force_generic_plan")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::raw_sql(&format!("PREPARE {name}({types}) AS {sql}"))
        .execute(&mut *tx)
        .await
        .unwrap();
    let plan: Value = sqlx::query_scalar(&format!("{opts} EXECUTE {name}({args})"))
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    canon(pool, plan).await
}

/// Bind values for [`explain_custom`].
enum Binds {
    Claim(i64),
    HeldBefore(String, i32, chrono::DateTime<chrono::Utc>, String),
    Reconcile(chrono::DateTime<chrono::Utc>, i64),
}

type Q<'q> = QueryScalar<'q, Postgres, Value, PgArguments>;

impl Binds {
    fn apply<'q>(self, q: Q<'q>) -> Q<'q> {
        let statuses: Vec<String> = lifecycle::HOLDING_STATUSES
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        match self {
            Binds::Claim(n) => q.bind(n).bind(Vec::<String>::new()).bind(statuses),
            Binds::HeldBefore(g, s, c, i) => q.bind(g).bind(s).bind(c).bind(i).bind(statuses),
            Binds::Reconcile(at, n) => q.bind(at).bind(n),
        }
    }
}

fn statement(name: &str) -> &'static str {
    lifecycle::queue_statements()
        .into_iter()
        .find(|(n, _)| *n == name)
        .unwrap()
        .1
}

// ─── The test ───────────────────────────────────────────────────────────────

async fn plans_in(stats: Stats, queued: i64, pending: i64, claim_only: bool) {
    let strict_sweeps = queued == QUEUED;
    let (_db, url): (TestDb, String) = start_db("plan").await;
    let pool = create_pool(&url).await.unwrap();
    run_migrations(&pool, MigrationProfile::Production)
        .await
        .unwrap();
    let started = Instant::now();
    let sizes = seed(&pool, stats, queued, pending).await;
    // The queue rows a claim can have to skip besides the claimed head: the
    // BLOCK_ON_ERROR rows of groups a FAILED job holds, or that have a job
    // in a retry backoff (an upper bound: the later rows of such a group).
    let held_upper: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM msg_dispatch_queue q WHERE q.mode = 'BLOCK_ON_ERROR' \
           AND (q.message_group IN (SELECT message_group FROM msg_dispatch_jobs \
                                     WHERE status = 'FAILED') \
             OR q.message_group IN (SELECT message_group FROM msg_dispatch_queue \
                                     WHERE scheduled_for > now()))",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    println!("[{stats:?}] seeded in {:?}", started.elapsed());
    assert_eq!(
        lifecycle::queue_drift(&pool)
            .await
            .unwrap()
            .missing_or_stale,
        0
    );

    let mut report: Vec<String> = Vec::new();
    let now = chrono::Utc::now();

    // ── the claim ──────────────────────────────────────────────────────
    let sql = statement("claim");
    let claim_args = "500, ARRAY[]::text[], ARRAY['FAILED','ERROR']::text[]";
    let custom = explain_custom(&pool, sql, true, Binds::Claim(500)).await;
    let generic = explain_generic(
        &pool,
        "p_claim",
        sql,
        "bigint, text[], text[]",
        claim_args,
        true,
    )
    .await;
    for (kind, plan) in [("claim custom", &custom), ("claim generic", &generic)] {
        report.push(summary(kind, plan));
        // With no usable statistics and a SMALL queue the planner rightly
        // prefers a seq scan plus a sort (the queue is a few dozen pages: it
        // took 25-30 ms in the run that found this); a deep queue, and
        // every queue with statistics, must walk the index.
        let small_without_stats = !matches!(stats, Stats::Analysed) && sizes.pending < 20_000;
        if small_without_stats {
            let exec = plan[0]["Execution Time"].as_f64().unwrap_or(0.0);
            assert!(
                exec < 250.0,
                "{stats:?} {kind}: the claim took {exec} ms: {}",
                compact(plan)
            );
            continue;
        }
        assert!(
            indexes(plan).contains("idx_dispatch_queue_order"),
            "{stats:?} {kind}: the claim must walk idx_dispatch_queue_order: {}",
            compact(plan)
        );
        assert!(
            !node_types(plan).contains("Sort"),
            "{stats:?} {kind}: the claim must not sort: {}",
            compact(plan)
        );
        assert!(
            !seq_scans(plan)
                .iter()
                .any(|r| r.starts_with("msg_dispatch_jobs")),
            "{stats:?} {kind}: the claim must not seq-scan msg_dispatch_jobs: {}",
            compact(plan)
        );
        // Stops early: the queue index scan reads about LIMIT + the rows it
        // skips (800 claimed at the head, the backoff holders' successors),
        // not the whole 20,000.
        let scanned: i64 = nodes(plan)
            .iter()
            .filter(|n| text(n, "Index Name") == "idx_dispatch_queue_order")
            .filter(|n| text(n, "Parent Relationship") != "SubPlan")
            .map(|n| {
                let rows = n["Actual Rows"].as_f64().unwrap_or(0.0);
                let loops = n["Actual Loops"].as_f64().unwrap_or(1.0);
                let removed = n["Rows Removed by Filter"].as_f64().unwrap_or(0.0);
                ((rows + removed) * loops.max(1.0)) as i64
            })
            .next()
            .unwrap_or(i64::MAX);
        let budget = 500 + sizes.claimed_head + held_upper + 100;
        assert!(
            scanned <= budget,
            "{stats:?} {kind}: the claim read {scanned} queue rows (budget {budget} = LIMIT + \
             {} claimed + at most {held_upper} held; {} PENDING): it must stop early: {}",
            sizes.claimed_head,
            sizes.pending,
            compact(plan)
        );
    }

    if claim_only {
        println!("[{stats:?}] {pending} PENDING:\n  {}", report.join("\n  "));
        return;
    }

    // ── the delivery-time hold-back ────────────────────────────────────
    let sql = statement("group_held_before");
    let held_args = "'g0123', 10, now(), 'J000000000000', ARRAY['FAILED','ERROR']::text[]";
    let custom = explain_custom(
        &pool,
        sql,
        true,
        Binds::HeldBefore("g0123".into(), 10, now, "J000000000000".into()),
    )
    .await;
    let generic = explain_generic(
        &pool,
        "p_held",
        sql,
        "text, int, timestamptz, text, text[]",
        held_args,
        true,
    )
    .await;
    for (kind, plan) in [
        ("group_held_before custom", &custom),
        ("group_held_before generic", &generic),
    ] {
        report.push(summary(kind, plan));
        let idx = indexes(plan);
        assert!(
            idx.contains("idx_dispatch_jobs_status_group"),
            "{stats:?} {kind}: the FAILED/ERROR lookup must use idx_dispatch_jobs_status_group: {}",
            compact(plan)
        );
        assert!(
            idx.contains("idx_dispatch_queue_order"),
            "{stats:?} {kind}: the backoff lookup must use the queue's order index: {}",
            compact(plan)
        );
        assert!(
            seq_scans(plan).is_empty(),
            "{stats:?} {kind}: nothing may seq-scan: {}",
            compact(plan)
        );
    }

    // ── the reconcile sweep ────────────────────────────────────────────
    for (name, args_generic, binds) in [
        (
            "reconcile_insert",
            "now(), 5000",
            Binds::Reconcile(now, 5000),
        ),
        (
            "reconcile_delete",
            "now(), 5000",
            Binds::Reconcile(now, 5000),
        ),
        (
            "reconcile_refresh",
            "now(), 5000",
            Binds::Reconcile(now, 5000),
        ),
    ] {
        let sql = statement(name);
        let custom = explain_custom(&pool, sql, true, binds).await;
        let generic = explain_generic(
            &pool,
            &format!("p_{name}"),
            sql,
            "timestamptz, bigint",
            args_generic,
            true,
        )
        .await;
        for (kind, plan) in [("custom", &custom), ("generic", &generic)] {
            report.push(summary(&format!("{name} {kind}"), plan));
            // PENDING jobs are read through the plain status index, or by
            // primary key; never by scanning a partition.
            assert!(
                !seq_scans(plan)
                    .iter()
                    .any(|r| r.starts_with("msg_dispatch_jobs")),
                "{stats:?} {name} {kind}: must not seq-scan msg_dispatch_jobs: {}",
                compact(plan)
            );
            if name == "reconcile_insert" {
                assert!(
                    indexes(plan).contains("idx_dispatch_jobs_status_group"),
                    "{stats:?} {name} {kind}: PENDING must be read through idx_dispatch_jobs_status_group: {}",
                    compact(plan)
                );
            }
        }
    }

    // ── the sweeps: stale recovery, the reaper, the callback's guards ─────
    for which in [
        "stale_queued",
        "stale_processing",
        "reap",
        "mark_queued",
        "claim_for_delivery",
        "schedule_retry",
    ] {
        let mut tx = pool.begin().await.unwrap();
        let plan = lifecycle::explain_statement(&mut *tx, which, true)
            .await
            .unwrap();
        tx.rollback().await.unwrap();
        let plan = canon(&pool, plan).await;
        report.push(summary(which, &plan));
        if !strict_sweeps && matches!(which, "stale_queued" | "stale_processing" | "reap") {
            println!("[{stats:?}] {which} at {queued} QUEUED:{}", compact(&plan));
            continue;
        }
        assert!(
            !seq_scans(&plan)
                .iter()
                .any(|r| r.starts_with("msg_dispatch_jobs")),
            "{stats:?} {which}: must not seq-scan msg_dispatch_jobs: {}",
            compact(&plan)
        );
        if matches!(which, "stale_queued" | "stale_processing" | "reap") {
            assert!(
                indexes(&plan).contains("idx_dispatch_jobs_status_group"),
                "{stats:?} {which}: the status sweep must use idx_dispatch_jobs_status_group: {}",
                compact(&plan)
            );
        }
    }

    println!("[{stats:?}]\n  {}", report.join("\n  "));
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn plans_when_freshly_analysed() {
    plans_in(Stats::Analysed, QUEUED, PENDING, false).await;
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn plans_when_analysed_while_empty() {
    plans_in(Stats::AnalysedWhileEmpty, QUEUED, PENDING, false).await;
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn plans_when_never_analysed() {
    plans_in(Stats::NeverAnalysed, QUEUED, PENDING, false).await;
}

/// The sweeps at 100,000 QUEUED rows (a table of 400,000): the plan and the
/// time are printed, and the sweeps must still not touch more than they have
/// to: a seq scan is the planner's right choice when a quarter of the table
/// is QUEUED, so only the result is checked here.
#[tokio::test]
#[ignore = "requires Docker"]
async fn sweeps_at_100k_queued() {
    plans_in(Stats::Analysed, QUEUED_AT_SCALE, PENDING, false).await;
}

/// A deep queue and no usable statistics: the claim must still walk the order
/// index and stop early (a seq scan plus a sort of the whole queue every
/// second is what an unlucky plan costs).
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_claim_stops_early_in_a_deep_queue_without_statistics() {
    plans_in(Stats::NeverAnalysed, QUEUED, PENDING_DEEP, true).await;
    plans_in(Stats::AnalysedWhileEmpty, QUEUED, PENDING_DEEP, true).await;
}
