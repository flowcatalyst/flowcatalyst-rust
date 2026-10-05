//! Query plans of the dispatch queue's reads and writes, against a real
//! PostgreSQL, on the scheduler's own pool (`plan_cache_mode =
//! force_custom_plan`, `enable_sort = off`).
//!
//! 1. `claim_plans_*`: the claim's two statements (S1 the ordered SELECT, S2
//!    the primary-key DELETE) on a queue table of about 5,000 and 100,000
//!    rows, in the states a production queue can be in: never analysed;
//!    analysed while brand-new and empty; DRAINED then ANALYZEd with its
//!    pages still allocated, then a burst; freshly analysed; analysed when
//!    every row was scheduled for the future. Shape only: S1 has no Sort and
//!    no Seq Scan; S2 is an index scan (the primary key, or in the drained
//!    state the order index) and never a Seq Scan at 100,000 rows.
//! 2. `the_cached_plan_*`: statements prepared and run several times on an
//!    empty, vacuumed queue on ONE connection, then a burst, then the claim
//!    on that same connection. With the settings the plan is the good one;
//!    without them (negative control) the cached plan is printed.
//! 3. `plans_when_*`: a 910,000-row jobs table (see `seed`) in three
//!    statistics states: the restore (jobs by primary key), the hold-back
//!    queries, the reconcile statements and the sweeps do not seq-scan
//!    `msg_dispatch_jobs`.
//!
//! `-- --nocapture` prints every plan summary and its timing. Requires
//! Docker, or a local PostgreSQL (`FC_TEST_PG_BIN`).

use std::collections::BTreeSet;
use std::time::Instant;

use serde_json::Value;
use sqlx::postgres::{PgArguments, PgConnectOptions};
use sqlx::query::QueryScalar;
use sqlx::{Connection, PgConnection, PgPool, Postgres};
use std::str::FromStr;

use crate::support::{start_db, TestDb};
use fc_platform::dispatch_job::lifecycle;
use fc_platform::shared::database::{
    create_pool, create_scheduler_pool, run_migrations, with_scheduler_planner_options,
    MigrationProfile,
};

/// Rows by status in the seeded table.
const COMPLETED: i64 = 900_000;
/// QUEUED rows in the plan tests (a busy table); the sweeps are also timed at
/// 100,000.
const QUEUED: i64 = 5_000;
const QUEUED_AT_SCALE: i64 = 100_000;
const PENDING: i64 = 5_000;

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
    if matches!(stats, Stats::Analysed) {
        exec(
            pool,
            "ANALYZE msg_dispatch_jobs; ANALYZE msg_dispatch_queue",
        )
        .await;
    }
    TestDbSizes { pending }
}

struct TestDbSizes {
    pending: i64,
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

/// Bind values for [`explain_custom`].
enum Binds {
    ClaimSelect(i64),
    ClaimDelete(Vec<String>),
    Restore(Vec<String>, Vec<chrono::DateTime<chrono::Utc>>),
    GroupHolders(
        Vec<String>,
        Vec<i32>,
        Vec<chrono::DateTime<chrono::Utc>>,
        Vec<String>,
    ),
    HeldBefore(String, i32, chrono::DateTime<chrono::Utc>, String),
    ReconcileInsert(chrono::DateTime<chrono::Utc>, i64),
    Limit(i64),
}

type Q<'q> = QueryScalar<'q, Postgres, Value, PgArguments>;

impl Binds {
    fn apply<'q>(self, q: Q<'q>) -> Q<'q> {
        let statuses: Vec<String> = lifecycle::HOLDING_STATUSES
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        match self {
            Binds::ClaimSelect(n) => q
                .bind(n)
                .bind(Vec::<String>::new())
                .bind(Vec::<String>::new()),
            Binds::ClaimDelete(ids) => q.bind(ids),
            Binds::Restore(ids, created) => q.bind(ids).bind(created),
            Binds::GroupHolders(groups, seqs, created, ids) => q
                .bind(statuses)
                .bind(groups)
                .bind(seqs)
                .bind(created)
                .bind(ids),
            Binds::HeldBefore(g, s, c, i) => q.bind(g).bind(s).bind(c).bind(i).bind(statuses),
            Binds::ReconcileInsert(at, n) => q.bind(at).bind(n).bind(Vec::<String>::new()),
            Binds::Limit(n) => q.bind(n),
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

// ─── The claim on a queue-only table ────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
enum QState {
    NeverAnalysed,
    AnalysedWhileEmpty,
    DrainedThenAnalysed,
    FreshlyAnalysed,
    AnalysedAllFuture,
}

fn insert_queue(n: i64, future: bool) -> String {
    let sched = if future {
        "now() + interval '1 day'"
    } else {
        "NULL"
    };
    format!(
        "INSERT INTO msg_dispatch_queue (job_id, job_created_at, message_group, sequence, \
                scheduled_for, mode, version) \
         SELECT 'Q' || lpad(n::text, 12, '0'), now() - (n % 100000) * interval '1 second', \
                CASE WHEN n % 7 = 0 THEN NULL ELSE 'g' || lpad((n % 500)::text, 4, '0') END, \
                (n % 40)::int, {sched}, 'IMMEDIATE', now() \
           FROM generate_series(1, {n}) n"
    )
}

/// A migrated database with its scheduler pool, autovacuum off on the queue
/// (the statistics stay what the test made them).
async fn queue_db() -> (TestDb, String, PgPool) {
    let (db, url) = start_db("plan").await;
    let pool = create_pool(&url).await.unwrap();
    run_migrations(&pool, MigrationProfile::Production)
        .await
        .unwrap();
    exec(
        &pool,
        "ALTER TABLE msg_dispatch_queue SET (autovacuum_enabled = false)",
    )
    .await;
    let scheduler = create_scheduler_pool(&url, 2).await.unwrap();
    (db, url, scheduler)
}

async fn put_queue_in_state(pool: &PgPool, state: QState, n: i64) {
    match state {
        QState::NeverAnalysed => exec(pool, &insert_queue(n, false)).await,
        QState::AnalysedWhileEmpty => {
            exec(pool, "ANALYZE msg_dispatch_queue").await;
            exec(pool, &insert_queue(n, false)).await;
        }
        QState::DrainedThenAnalysed => {
            exec(pool, &insert_queue(n, false)).await;
            exec(pool, "DELETE FROM msg_dispatch_queue").await;
            // Pages stay allocated (no VACUUM); the planner then believes
            // the table has one row.
            exec(pool, "ANALYZE msg_dispatch_queue").await;
            exec(pool, &insert_queue(n, false)).await;
        }
        QState::FreshlyAnalysed => {
            exec(pool, &insert_queue(n, false)).await;
            exec(pool, "ANALYZE msg_dispatch_queue").await;
        }
        QState::AnalysedAllFuture => {
            exec(pool, &insert_queue(n, true)).await;
            exec(pool, "ANALYZE msg_dispatch_queue").await;
            exec(pool, "UPDATE msg_dispatch_queue SET scheduled_for = NULL").await;
        }
    }
}

/// The shape of the claim's two statements in `state` with `n` queue rows.
async fn claim_plans(state: QState, n: i64) {
    let (_db, url, spool) = queue_db().await;
    put_queue_in_state(&spool, state, n).await;

    let s1 = explain_custom(
        &spool,
        statement("claim_select"),
        true,
        Binds::ClaimSelect(500),
    )
    .await;
    let ids: Vec<String> = sqlx::query_scalar(statement("claim_select"))
        .bind(500_i64)
        .bind(Vec::<String>::new())
        .bind(Vec::<String>::new())
        .fetch_all(&spool)
        .await
        .unwrap();
    assert_eq!(ids.len(), 500);
    let s2 = explain_custom(
        &spool,
        statement("claim_delete"),
        true,
        Binds::ClaimDelete(ids),
    )
    .await;
    println!(
        "[{state:?} @ {n}] {}\n[{state:?} @ {n}] {}",
        summary("S1", &s1),
        summary("S2", &s2)
    );

    // The hold-back's queue half: one ordered index probe per candidate
    // group, however deep the queue is (scheduler pool); and the
    // delivery-time check on the PLATFORM pool, which has no planner settings.
    // The data shape: 500 groups, each with ~200 DUE rows and none scheduled
    // for the future; the probe is bounded by each group's last candidate
    // (here sequence 1, the head of the group).
    let groups: Vec<String> = (0..500).map(|g| format!("g{g:04}")).collect();
    let holders = explain_custom(
        &spool,
        statement("group_holders"),
        true,
        Binds::GroupHolders(
            groups.clone(),
            vec![1; groups.len()],
            vec![chrono::Utc::now(); groups.len()],
            vec!["Q999999999999".to_string(); groups.len()],
        ),
    )
    .await;
    let platform = create_pool(&url).await.unwrap();
    let before = explain_custom(
        &platform,
        statement("group_held_before"),
        true,
        Binds::HeldBefore(
            "g0123".into(),
            20,
            chrono::Utc::now(),
            "Q999999999999".into(),
        ),
    )
    .await;
    println!(
        "[{state:?} @ {n}] {}\n[{state:?} @ {n}] {} (platform pool)",
        summary("group_holders", &holders),
        summary("group_held_before", &before)
    );
    assert!(
        seq_scans(&holders).is_empty() && indexes(&holders).contains("idx_dispatch_queue_order"),
        "{state:?} @ {n}: the holders' queue half must probe the order index: {}",
        compact(&holders)
    );
    assert!(
        seq_scans(&before).is_empty(),
        "{state:?} @ {n}: group_held_before (platform pool) must not seq-scan: {}",
        compact(&before)
    );
    assert!(
        !node_types(&s1).contains("Sort"),
        "{state:?} @ {n}: S1 must not sort: {}",
        compact(&s1)
    );
    assert!(
        seq_scans(&s1).is_empty(),
        "{state:?} @ {n}: S1 must not seq-scan: {}",
        compact(&s1)
    );
    assert!(
        indexes(&s1).contains("idx_dispatch_queue_order"),
        "{state:?} @ {n}: S1 must walk the order index: {}",
        compact(&s1)
    );
    // At 100,000 rows S2 must use an index (the primary key; only in the
    // drained state may the order index stand in). A 5,000-row queue is a few
    // dozen pages: a seq scan of it costs about what the probes would, and the
    // spec only forbids it at 100,000.
    if n >= 100_000 {
        let used = indexes(&s2);
        let allowed = ["msg_dispatch_queue_pkey", "idx_dispatch_queue_order"];
        assert!(
            !used.is_empty() && used.iter().all(|i| allowed.contains(&i.as_str())),
            "{state:?} @ {n}: S2 must be an index scan: {}",
            compact(&s2)
        );
        if !matches!(state, QState::DrainedThenAnalysed) {
            assert!(
                used.contains("msg_dispatch_queue_pkey"),
                "{state:?} @ {n}: S2 must use the primary key: {}",
                compact(&s2)
            );
        }
        assert!(
            seq_scans(&s2).is_empty(),
            "{state:?} @ {n}: S2 must never seq-scan at {n} rows: {}",
            compact(&s2)
        );
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn claim_plans_never_analysed() {
    for n in [5_000, 100_000] {
        claim_plans(QState::NeverAnalysed, n).await;
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn claim_plans_analysed_while_empty() {
    for n in [5_000, 100_000] {
        claim_plans(QState::AnalysedWhileEmpty, n).await;
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn claim_plans_drained_then_analysed_then_a_burst() {
    for n in [5_000, 100_000] {
        claim_plans(QState::DrainedThenAnalysed, n).await;
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn claim_plans_freshly_analysed() {
    for n in [5_000, 100_000] {
        claim_plans(QState::FreshlyAnalysed, n).await;
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn claim_plans_analysed_when_every_row_was_scheduled_for_the_future() {
    for n in [5_000, 100_000] {
        claim_plans(QState::AnalysedAllFuture, n).await;
    }
}

// ─── The cached plan ────────────────────────────────────────────────────────

/// Statements prepared and run several times on an empty, vacuumed queue on
/// ONE connection, then a burst of `n` rows, then the claim's two statements
/// on that same connection (the plan cache decides which plan they get).
/// Returns S1's and S2's plans.
async fn cached_plan_after_a_burst(with_settings: bool, n: i64) -> (Value, Value) {
    let (_db, url, _spool) = queue_db().await;
    let mut opts = PgConnectOptions::from_str(&url).unwrap();
    if with_settings {
        opts = with_scheduler_planner_options(opts);
    }
    let mut conn = PgConnection::connect_with(&opts).await.unwrap();
    let run = |sql: String| sqlx::raw_sql(Box::leak(sql.into_boxed_str()));
    run("VACUUM (ANALYZE) msg_dispatch_queue".into())
        .execute(&mut conn)
        .await
        .unwrap();
    run(format!(
        "PREPARE s1(bigint, text[], text[]) AS {}",
        statement("claim_select")
    ))
    .execute(&mut conn)
    .await
    .unwrap();
    run(format!(
        "PREPARE s2(text[]) AS {}",
        statement("claim_delete")
    ))
    .execute(&mut conn)
    .await
    .unwrap();
    for _ in 0..8 {
        run("EXECUTE s1(500, ARRAY[]::text[], ARRAY[]::text[])".into())
            .execute(&mut conn)
            .await
            .unwrap();
        run("EXECUTE s2(ARRAY['none'])".into())
            .execute(&mut conn)
            .await
            .unwrap();
    }
    // The burst, on another connection (no ANALYZE: autovacuum is off).
    let burst = PgConnection::connect_with(&PgConnectOptions::from_str(&url).unwrap())
        .await
        .unwrap();
    let mut burst = burst;
    run(insert_queue(n, false))
        .execute(&mut burst)
        .await
        .unwrap();

    let s1: Value = sqlx::query_scalar(
        "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) EXECUTE s1(500, ARRAY[]::text[], ARRAY[]::text[])",
    )
    .fetch_one(&mut conn)
    .await
    .unwrap();
    let ids: Vec<String> = (1..=500).map(|i| format!("Q{i:012}")).collect();
    let list = ids
        .iter()
        .map(|i| format!("'{i}'"))
        .collect::<Vec<_>>()
        .join(",");
    run("BEGIN".into()).execute(&mut conn).await.unwrap();
    let s2: Value = sqlx::query_scalar(&format!(
        "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) EXECUTE s2(ARRAY[{list}])"
    ))
    .fetch_one(&mut conn)
    .await
    .unwrap();
    run("ROLLBACK".into()).execute(&mut conn).await.unwrap();
    (s1, s2)
}

/// With the scheduler's settings a plan cached while the queue was empty is
/// not reused badly after a burst: S1 still walks the order index with no
/// Sort, S2 uses the primary key.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_cached_plan_after_a_burst_is_good_with_the_schedulers_settings() {
    let (s1, s2) = cached_plan_after_a_burst(true, 100_000).await;
    println!(
        "[cached, settings] {}\n[cached, settings] {}",
        summary("S1", &s1),
        summary("S2", &s2)
    );
    assert!(!node_types(&s1).contains("Sort"), "{}", compact(&s1));
    assert!(seq_scans(&s1).is_empty(), "{}", compact(&s1));
    assert!(seq_scans(&s2).is_empty(), "{}", compact(&s2));
}

/// NEGATIVE CONTROL: the same sequence on a connection WITHOUT the two
/// settings. Whether it reproduces the bad cached plan on this PostgreSQL is
/// printed; the test only requires that the good case above stays good.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_cached_plan_after_a_burst_without_the_settings_negative_control() {
    let (s1, s2) = cached_plan_after_a_burst(false, 100_000).await;
    let bad1 = node_types(&s1).contains("Sort") || !seq_scans(&s1).is_empty();
    let bad2 = !seq_scans(&s2).is_empty();
    println!(
        "[cached, NO settings] bad S1 plan: {bad1}, bad S2 plan: {bad2}\n{}\n{}",
        summary("S1", &s1),
        summary("S2", &s2)
    );
}

// ─── The jobs table ─────────────────────────────────────────────────────────

async fn plans_in(stats: Stats, queued: i64, pending: i64) {
    let strict_sweeps = queued == QUEUED;
    let (_db, url): (TestDb, String) = start_db("plan").await;
    let pool = create_pool(&url).await.unwrap();
    run_migrations(&pool, MigrationProfile::Production)
        .await
        .unwrap();
    let started = Instant::now();
    let sizes = seed(&pool, stats, queued, pending).await;
    println!("[{stats:?}] seeded in {:?}", started.elapsed());
    assert_eq!(
        lifecycle::queue_drift(&pool)
            .await
            .unwrap()
            .missing_or_stale,
        0
    );
    // Every statement below runs on the scheduler's pool.
    let pool = create_scheduler_pool(&url, 3).await.unwrap();

    let mut report: Vec<String> = Vec::new();
    let now = chrono::Utc::now();

    // ── the restore: the claimed jobs read from the job table ─────────────
    let pending_rows: Vec<(String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT id, created_at FROM msg_dispatch_jobs WHERE status = 'PENDING' LIMIT 500",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(pending_rows.len() >= 500.min(sizes.pending as usize));
    let ids: Vec<String> = pending_rows.iter().map(|r| r.0.clone()).collect();
    let created: Vec<chrono::DateTime<chrono::Utc>> = pending_rows.iter().map(|r| r.1).collect();
    let plan = explain_custom(
        &pool,
        statement("restore"),
        true,
        Binds::Restore(ids, created),
    )
    .await;
    report.push(summary("restore", &plan));
    assert!(
        indexes(&plan).contains("msg_dispatch_jobs_pkey")
            && !indexes(&plan).contains("idx_dispatch_jobs_status_group"),
        "{stats:?} restore must read the jobs by primary key only: {}",
        compact(&plan)
    );
    assert!(
        !seq_scans(&plan)
            .iter()
            .any(|r| r.starts_with("msg_dispatch_jobs")),
        "{stats:?} restore must not seq-scan the jobs: {}",
        compact(&plan)
    );

    // ── the batched hold-back after the claim: the earliest holder per group ──
    let groups: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT message_group FROM msg_dispatch_queue \
          WHERE mode = 'BLOCK_ON_ERROR' AND message_group IS NOT NULL LIMIT 500",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let plan = explain_custom(
        &pool,
        statement("group_holders"),
        true,
        Binds::GroupHolders(
            groups.clone(),
            vec![1; groups.len()],
            vec![chrono::Utc::now(); groups.len()],
            vec!["Q999999999999".to_string(); groups.len()],
        ),
    )
    .await;
    report.push(summary("group_holders", &plan));
    assert!(
        indexes(&plan).contains("idx_dispatch_jobs_status_group"),
        "{stats:?} the FAILED/ERROR lookup must use idx_dispatch_jobs_status_group: {}",
        compact(&plan)
    );
    assert!(
        !seq_scans(&plan)
            .iter()
            .any(|r| r.starts_with("msg_dispatch_jobs")),
        "{stats:?} group_holders must not seq-scan the jobs: {}",
        compact(&plan)
    );

    // ── the delivery-time hold-back ────────────────────────────────────
    let plan = explain_custom(
        &pool,
        statement("group_held_before"),
        true,
        Binds::HeldBefore("g0123".into(), 10, now, "J000000000000".into()),
    )
    .await;
    report.push(summary("group_held_before", &plan));
    assert!(
        indexes(&plan).contains("idx_dispatch_jobs_status_group"),
        "{stats:?} {}",
        compact(&plan)
    );
    assert!(seq_scans(&plan).is_empty(), "{stats:?} {}", compact(&plan));

    // ── the reconcile sweep ────────────────────────────────────────────
    for (name, binds) in [
        ("reconcile_insert", Binds::ReconcileInsert(now, 5000)),
        ("reconcile_delete", Binds::Limit(5000)),
        ("reconcile_refresh", Binds::Limit(5000)),
    ] {
        let plan = explain_custom(&pool, statement(name), true, binds).await;
        report.push(summary(name, &plan));
        assert!(
            !seq_scans(&plan)
                .iter()
                .any(|r| r.starts_with("msg_dispatch_jobs")),
            "{stats:?} {name}: must not seq-scan msg_dispatch_jobs: {}",
            compact(&plan)
        );
        if name == "reconcile_insert" {
            assert!(
                indexes(&plan).contains("idx_dispatch_jobs_status_group"),
                "{stats:?} {name}: PENDING must be read through idx_dispatch_jobs_status_group: {}",
                compact(&plan)
            );
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
    plans_in(Stats::Analysed, QUEUED, PENDING).await;
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn plans_when_analysed_while_empty() {
    plans_in(Stats::AnalysedWhileEmpty, QUEUED, PENDING).await;
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn plans_when_never_analysed() {
    plans_in(Stats::NeverAnalysed, QUEUED, PENDING).await;
}

/// The sweeps at 100,000 QUEUED rows (a table of 1,000,000): the plan and the
/// time are printed (a seq scan is the planner's right choice when a tenth of
/// the table is QUEUED).
#[tokio::test]
#[ignore = "requires Docker"]
async fn sweeps_at_100k_queued() {
    plans_in(Stats::Analysed, QUEUED_AT_SCALE, PENDING).await;
}
