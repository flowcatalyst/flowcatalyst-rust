//! Query plans of the scheduler's claim, mark-QUEUED, hold-back, stale sweeps
//! and backlog sample against a real PostgreSQL with the scheduler's two
//! planner settings (`plan_cache_mode = force_custom_plan`, `enable_sort =
//! off`), on a job table of several hundred thousand mostly-COMPLETED rows
//! over several monthly partitions, with 5,000 and 200,000 PENDING jobs, in
//! the statistics states a production table can be in:
//!
//! * freshly analysed;
//! * never analysed;
//! * the active partition analysed while it held NO pending job, then a burst
//!   (statistics say PENDING is absent);
//! * the empty forward/back partitions VACUUMed and ANALYZEd and the populated
//!   ones never analysed;
//! * every pending row sharing one `created_at` and one `sequence` (ties);
//! * the cached-plan case (the statements run several times on ONE connection
//!   while nothing is pending, then the burst, then the same connection);
//! * in-flight arrays of 0, 1,000 and 5,000 ids that are the FIRST rows of the
//!   walk.
//!
//! Shape is asserted, not time: the claim has no Sort and no Seq Scan and
//! walks `idx_dispatch_jobs_status_group`; mark-QUEUED reads by primary key
//! and never walks the status index (it fails if its status guard is made
//! sargable again); hold-back and the sweeps use the plain index. The
//! timings are printed (`-- --nocapture`). Requires Docker, or a local
//! PostgreSQL (`FC_TEST_PG_BIN`).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::postgres::{PgArguments, PgConnectOptions, PgPoolOptions};
use sqlx::query::QueryScalar;
use sqlx::{PgPool, Postgres};
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use crate::support::{start_db, TestDb};
use fc_platform::dispatch_job::lifecycle;
use fc_platform::scheduler::{
    DispatchAuthService, DispatchPublisher, DispatchScheduler, PoolCodeResolver, PublishItem,
    PublishOutcome, SchedulerConfig,
};
use fc_platform::shared::database::{
    create_pool, create_scheduler_pool, run_migrations, with_scheduler_planner_options,
    MigrationProfile,
};

/// Seeded rows besides the pending ones.
const COMPLETED: i64 = 400_000;
const QUEUED: i64 = 2_000;
const FAILED: i64 = 100;
const PROCESSING: i64 = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    FreshlyAnalysed,
    NeverAnalysed,
    ActiveAnalysedBeforeTheBurst,
    EmptyPartitionsVacuumed,
    Ties,
    /// State 3 with `sequence = 1` and `created_at` increasing with the id:
    /// the order inside a group is the order of the ids.
    ActiveAnalysedBeforeTheOrderedBurst,
}

async fn exec(pool: &PgPool, sql: &str) {
    sqlx::raw_sql(sql)
        .execute(pool)
        .await
        .unwrap_or_else(|e| panic!("{e}: {sql}"));
}

/// Insert `count` jobs of `status` with ids `prefix || n`, created within the
/// last month and the next two (so four partitions are populated).
fn load(prefix: &str, from: i64, count: i64, status: &str, extra: &str, created: &str) -> String {
    format!(
        "INSERT INTO msg_dispatch_jobs (id, code, source, target_url, payload, status, mode, \
             message_group, sequence, created_at, updated_at, subscription_id, scheduled_for) \
         SELECT '{prefix}' || lpad(n::text, 12, '0'), 'a:b:c:d', 's', 'http://x/h', '{{}}', \
                '{status}', {extra} \
           FROM (SELECT n, {created} AS c FROM generate_series({from}, {from} + {count} - 1) n) s"
    )
}

const SPREAD: &str =
    "date_trunc('month', now()) - interval '25 days' + (random() * interval '95 days')";
/// The active partition only (the current month, in the past).
const ACTIVE: &str = "date_trunc('month', now()) + n * interval '1 second' / 100";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Burst {
    Spread,
    Ties,
    Ordered,
}

async fn disable_autovacuum(pool: &PgPool) {
    exec(
        pool,
        "DO $$ DECLARE r record; BEGIN \
           FOR r IN SELECT inhrelid::regclass AS t FROM pg_inherits \
                     WHERE inhparent = 'msg_dispatch_jobs'::regclass LOOP \
             EXECUTE format('ALTER TABLE %s SET (autovacuum_enabled = false)', r.t); \
           END LOOP; END $$",
    )
    .await;
}

/// Everything but the pending jobs.
async fn seed_background(pool: &PgPool) {
    exec(
        pool,
        &load(
            "C",
            1,
            COMPLETED,
            "COMPLETED",
            "'IMMEDIATE', CASE WHEN n % 3 = 0 THEN NULL ELSE 'g' || lpad((n % 500)::text, 4, '0') END, \
             (n % 40)::int, c, c, NULL, NULL",
            SPREAD,
        ),
    )
    .await;
    exec(
        pool,
        &load(
            "Q",
            1,
            QUEUED,
            "QUEUED",
            "'BLOCK_ON_ERROR', 'g' || lpad((n % 500)::text, 4, '0'), (n % 40)::int, c, \
             CASE WHEN n % 10 = 0 THEN now() - interval '40 minutes' ELSE now() END, NULL, NULL",
            SPREAD,
        ),
    )
    .await;
    exec(
        pool,
        &load(
            "F",
            1,
            FAILED,
            "FAILED",
            "'BLOCK_ON_ERROR', 'g' || lpad((n % 500)::text, 4, '0'), (n % 40)::int, c, c, NULL, NULL",
            SPREAD,
        ),
    )
    .await;
    exec(
        pool,
        &load(
            "R",
            1,
            PROCESSING,
            "PROCESSING",
            "'IMMEDIATE', NULL, 0, c, CASE WHEN n % 10 = 0 THEN now() - interval '90 minutes' ELSE now() END, NULL, NULL",
            SPREAD,
        ),
    )
    .await;
}

/// The burst of `pending` PENDING jobs in the active partition: grouped and
/// ungrouped, BLOCK_ON_ERROR in groups some FAILED job holds, 1% in a backoff.
async fn burst(pool: &PgPool, pending: i64, shape: Burst) {
    let (seq, created) = match shape {
        Burst::Ties => ("1", "date_trunc('month', now()) + interval '1 hour'"),
        Burst::Spread => ("(n % 40)::int", ACTIVE),
        Burst::Ordered => ("1", ACTIVE),
    };
    exec(
        pool,
        &load(
            "P",
            1,
            pending,
            "PENDING",
            &format!(
                "CASE WHEN n % 5 = 0 THEN 'IMMEDIATE' ELSE 'BLOCK_ON_ERROR' END, \
                 CASE WHEN n % 5 = 0 THEN NULL ELSE 'g' || lpad((n % 500)::text, 4, '0') END, \
                 {seq}, c, c, CASE WHEN n % 7 = 0 THEN 'sub_x' END, \
                 CASE WHEN n % 100 = 0 THEN now() + interval '1 hour' END"
            ),
            created,
        ),
    )
    .await;
}

async fn prepare(pool: &PgPool, state: State, pending: i64) {
    disable_autovacuum(pool).await;
    match state {
        State::FreshlyAnalysed => {
            seed_background(pool).await;
            burst(pool, pending, Burst::Spread).await;
            exec(pool, "ANALYZE msg_dispatch_jobs").await;
        }
        State::NeverAnalysed => {
            seed_background(pool).await;
            burst(pool, pending, Burst::Spread).await;
        }
        State::ActiveAnalysedBeforeTheBurst => {
            seed_background(pool).await;
            exec(pool, "ANALYZE msg_dispatch_jobs").await;
            burst(pool, pending, Burst::Spread).await;
        }
        State::EmptyPartitionsVacuumed => {
            seed_background(pool).await;
            burst(pool, pending, Burst::Spread).await;
            let empty: Vec<String> = sqlx::query_scalar(
                "SELECT c.relname::text FROM pg_inherits i JOIN pg_class c ON c.oid = i.inhrelid \
                  WHERE i.inhparent = 'msg_dispatch_jobs'::regclass AND c.reltuples <= 0 \
                    AND NOT EXISTS (SELECT 1 FROM msg_dispatch_jobs j WHERE j.tableoid = c.oid)",
            )
            .fetch_all(pool)
            .await
            .unwrap();
            for t in empty {
                exec(pool, &format!("VACUUM (ANALYZE) \"{t}\"")).await;
            }
        }
        State::ActiveAnalysedBeforeTheOrderedBurst => {
            seed_background(pool).await;
            exec(pool, "ANALYZE msg_dispatch_jobs").await;
            burst(pool, pending, Burst::Ordered).await;
        }
        State::Ties => {
            seed_background(pool).await;
            burst(pool, pending, Burst::Ties).await;
            exec(pool, "ANALYZE msg_dispatch_jobs").await;
        }
    }
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
    Claim(i64, Vec<String>),
    GroupHolders(Vec<String>, Vec<i32>, Vec<DateTime<Utc>>, Vec<String>),
    HeldBefore(String, i32, DateTime<Utc>, String),
    None,
}

type Q<'q> = QueryScalar<'q, Postgres, Value, PgArguments>;

impl Binds {
    fn apply<'q>(self, q: Q<'q>) -> Q<'q> {
        let statuses: Vec<String> = lifecycle::HOLDING_STATUSES
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        match self {
            Binds::Claim(n, in_flight) => q
                .bind(n)
                .bind(Vec::<String>::new())
                .bind(Vec::<String>::new())
                .bind(in_flight),
            Binds::GroupHolders(groups, seqs, created, ids) => q
                .bind(statuses)
                .bind(groups)
                .bind(seqs)
                .bind(created)
                .bind(ids),
            Binds::HeldBefore(g, s, c, i) => q.bind(g).bind(s).bind(c).bind(i).bind(statuses),
            Binds::None => q,
        }
    }
}

fn statement(name: &str) -> &'static str {
    lifecycle::read_statements()
        .into_iter()
        .find(|(n, _)| *n == name)
        .unwrap()
        .1
}

// ─── The plans ──────────────────────────────────────────────────────────────

/// The claimed rows' `(id, created_at, version)`, as the lanes mark them.
fn keys(rows: &[lifecycle::ClaimRow]) -> Vec<(String, DateTime<Utc>, DateTime<Utc>)> {
    rows.iter()
        .map(|r| (r.job_id.clone(), r.job_created_at, r.version))
        .collect()
}

async fn mark_plan(pool: &PgPool, claimed: &[(String, DateTime<Utc>, DateTime<Utc>)]) -> Value {
    let mut tx = pool.begin().await.unwrap();
    let plan = lifecycle::explain_mark_queued(&mut *tx, claimed, true)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    canon(pool, plan).await
}

async fn plans_in(state: State, pending: i64) {
    let label = format!("{state:?} @ {pending} PENDING");
    let (_db, url): (TestDb, String) = start_db("plan").await;
    let platform = create_pool(&url).await.unwrap();
    run_migrations(&platform, MigrationProfile::Production)
        .await
        .unwrap();
    prepare(&platform, state, pending).await;
    // Every statement below runs on the scheduler's pool.
    let pool = create_scheduler_pool(&url, 3).await.unwrap();
    let mut report: Vec<String> = Vec::new();

    // ── the claim, with 0, 1,000 and 5,000 in-flight ids that are the FIRST
    //    rows of the walk
    let walk = lifecycle::claim(&pool, 5_000, &[], &[], &[]).await.unwrap();
    assert!(
        walk.len() >= 500,
        "{label}: the claim found {} rows",
        walk.len()
    );
    for k in [0usize, 1_000, 5_000] {
        let in_flight: Vec<String> = walk.iter().take(k).map(|r| r.job_id.clone()).collect();
        let plan = explain_custom(
            &pool,
            statement("claim"),
            true,
            Binds::Claim(500, in_flight),
        )
        .await;
        report.push(summary(&format!("claim (in flight {k})"), &plan));
        assert!(
            indexes(&plan).contains("idx_dispatch_jobs_status_group"),
            "{label}: the claim must walk idx_dispatch_jobs_status_group: {}",
            compact(&plan)
        );
        assert!(
            !node_types(&plan).contains("Sort"),
            "{label}: the claim must not sort: {}",
            compact(&plan)
        );
        assert!(
            seq_scans(&plan).is_empty(),
            "{label}: the claim must not seq-scan: {}",
            compact(&plan)
        );
    }

    // ── mark-QUEUED for the first 500 claimed: by primary key, never the
    //    status index (this fails if the status guard is made sargable again)
    let claimed = keys(&walk[..500]);
    let plan = mark_plan(&pool, &claimed).await;
    report.push(summary("mark_queued", &plan));
    assert!(
        indexes(&plan).contains("msg_dispatch_jobs_pkey")
            && !indexes(&plan).contains("idx_dispatch_jobs_status_group"),
        "{label}: mark-QUEUED must read by primary key and never walk the status index: {}",
        compact(&plan)
    );
    assert!(
        seq_scans(&plan).is_empty(),
        "{label}: mark-QUEUED must not seq-scan: {}",
        compact(&plan)
    );

    // ── hold-back: the earliest holder per group (claim time) ──────────────
    let mut groups: Vec<String> = walk
        .iter()
        .filter(|r| r.mode == "BLOCK_ON_ERROR")
        .filter_map(|r| r.message_group.clone())
        .collect();
    groups.sort();
    groups.dedup();
    let plan = explain_custom(
        &pool,
        statement("group_holders"),
        true,
        Binds::GroupHolders(
            groups.clone(),
            vec![1; groups.len()],
            vec![Utc::now(); groups.len()],
            vec!["Z999999999999".to_string(); groups.len()],
        ),
    )
    .await;
    report.push(summary("group_holders", &plan));
    assert!(
        indexes(&plan).contains("idx_dispatch_jobs_status_group") && seq_scans(&plan).is_empty(),
        "{label}: both hold-back halves must use the plain index: {}",
        compact(&plan)
    );
    // ... and at delivery time.
    let plan = explain_custom(
        &pool,
        statement("group_held_before"),
        true,
        Binds::HeldBefore("g0123".into(), 20, Utc::now(), "Z999999999999".into()),
    )
    .await;
    report.push(summary("group_held_before", &plan));
    assert!(
        indexes(&plan).contains("idx_dispatch_jobs_status_group") && seq_scans(&plan).is_empty(),
        "{label}: group_held_before must use the plain index: {}",
        compact(&plan)
    );

    // ── the sweeps ────────────────────────────────────────────────────────
    for which in ["stale_queued", "stale_processing", "reap"] {
        let mut tx = pool.begin().await.unwrap();
        let plan = lifecycle::explain_statement(&mut *tx, which, true)
            .await
            .unwrap();
        tx.rollback().await.unwrap();
        let plan = canon(&pool, plan).await;
        report.push(summary(which, &plan));
        assert!(
            indexes(&plan).contains("idx_dispatch_jobs_status_group")
                && seq_scans(&plan).is_empty(),
            "{label}: {which} must use the plain index: {}",
            compact(&plan)
        );
    }

    // ── the backlog sample: bounded by its LIMIT, so it may be an index scan
    //    or a sequential scan that stops after 100,001 rows; either way it
    //    never reads a huge backlog
    let plan = explain_custom(&pool, statement("backlog_count"), true, Binds::None).await;
    report.push(summary("backlog_count", &plan));
    let limit_rows = nodes(&plan)
        .iter()
        .find(|n| text(n, "Node Type") == "Limit")
        .and_then(|n| n["Actual Rows"].as_f64())
        .unwrap_or(f64::MAX);
    assert!(
        limit_rows <= 100_001.0 && plan[0]["Execution Time"].as_f64().unwrap_or(0.0) < 250.0,
        "{label}: backlog_count counted {limit_rows} rows: {}",
        compact(&plan)
    );
    let plan = explain_custom(&pool, statement("backlog_oldest"), true, Binds::None).await;
    report.push(summary("backlog_oldest", &plan));
    assert!(
        indexes(&plan).contains("idx_dispatch_jobs_status_group") && seq_scans(&plan).is_empty(),
        "{label}: backlog_oldest must use the plain index: {}",
        compact(&plan)
    );

    println!(
        "[{label}]\n  {}",
        report
            .iter()
            .map(|l| l.replace(['{', '}'], ""))
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

macro_rules! plan_tests {
    ($($name:ident: $state:expr, $pending:expr;)*) => {$(
        #[tokio::test]
        #[ignore = "requires Docker"]
        async fn $name() {
            plans_in($state, $pending).await;
        }
    )*};
}

plan_tests! {
    plans_freshly_analysed_5k: State::FreshlyAnalysed, 5_000;
    plans_freshly_analysed_200k: State::FreshlyAnalysed, 200_000;
    plans_never_analysed_5k: State::NeverAnalysed, 5_000;
    plans_never_analysed_200k: State::NeverAnalysed, 200_000;
    plans_active_partition_analysed_before_the_burst_5k: State::ActiveAnalysedBeforeTheBurst, 5_000;
    plans_active_partition_analysed_before_the_burst_200k: State::ActiveAnalysedBeforeTheBurst, 200_000;
    plans_empty_partitions_vacuumed_5k: State::EmptyPartitionsVacuumed, 5_000;
    plans_empty_partitions_vacuumed_200k: State::EmptyPartitionsVacuumed, 200_000;
    plans_with_ties_5k: State::Ties, 5_000;
    plans_with_ties_200k: State::Ties, 200_000;
}

/// NEGATIVE CONTROL: the same statements on a pool WITHOUT the two settings,
/// in the states where a plan can go wrong. Printed, not asserted: which
/// states go bad depends on the PostgreSQL version.
#[tokio::test]
#[ignore = "requires Docker"]
async fn negative_control_without_the_planner_settings() {
    for state in [
        State::ActiveAnalysedBeforeTheBurst,
        State::EmptyPartitionsVacuumed,
    ] {
        let (_db, url) = start_db("plan").await;
        let platform = create_pool(&url).await.unwrap();
        run_migrations(&platform, MigrationProfile::Production)
            .await
            .unwrap();
        prepare(&platform, state, 200_000).await;
        let walk = lifecycle::claim(&platform, 500, &[], &[], &[])
            .await
            .unwrap();
        let claim = explain_custom(
            &platform,
            statement("claim"),
            true,
            Binds::Claim(500, vec![]),
        )
        .await;
        let mark = mark_plan(&platform, &keys(&walk)).await;
        println!(
            "[NO settings, {state:?} @ 200000]\n  {}\n  {}",
            summary("claim", &claim),
            summary("mark_queued", &mark)
        );
    }
}

// ─── The cached plan ────────────────────────────────────────────────────────

/// The claim and mark-QUEUED are run several times on ONE connection while
/// nothing is pending (sqlx prepares and caches each statement per
/// connection), then the burst is inserted, then they run again on that same
/// connection. Returns `(claim ms, mark ms)` after the burst.
async fn cached_plan_after_a_burst(with_settings: bool) -> (f64, f64) {
    let (_db, url) = start_db("plan").await;
    let platform = create_pool(&url).await.unwrap();
    run_migrations(&platform, MigrationProfile::Production)
        .await
        .unwrap();
    disable_autovacuum(&platform).await;
    seed_background(&platform).await;
    exec(&platform, "VACUUM (ANALYZE) msg_dispatch_jobs").await;
    let mut opts = PgConnectOptions::from_str(&url).unwrap();
    if with_settings {
        opts = with_scheduler_planner_options(opts);
    }
    let one = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .unwrap();
    let ghost = vec![("P000000000000".to_string(), Utc::now(), Utc::now())];
    for _ in 0..8 {
        assert!(lifecycle::claim(&one, 500, &[], &[], &[])
            .await
            .unwrap()
            .is_empty());
        assert!(lifecycle::mark_queued(&one, &ghost)
            .await
            .unwrap()
            .is_empty());
    }
    burst(&platform, 200_000, Burst::Spread).await;
    let t = Instant::now();
    let walk = lifecycle::claim(&one, 500, &[], &[], &[]).await.unwrap();
    let claim_ms = t.elapsed().as_secs_f64() * 1e3;
    assert_eq!(walk.len(), 500);
    let t = Instant::now();
    let marked = lifecycle::mark_queued(&one, &keys(&walk)).await.unwrap();
    let mark_ms = t.elapsed().as_secs_f64() * 1e3;
    assert_eq!(marked.len(), 500);
    (claim_ms, mark_ms)
}

/// With the scheduler's settings a statement cached on an empty table is not
/// reused badly after the burst: both stay far under 250 ms.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_cached_plan_after_a_burst_is_good_with_the_schedulers_settings() {
    let (claim_ms, mark_ms) = cached_plan_after_a_burst(true).await;
    println!("[cached, settings] claim {claim_ms:.1} ms, mark-QUEUED {mark_ms:.1} ms");
    assert!(
        claim_ms < 250.0 && mark_ms < 250.0,
        "claim {claim_ms} ms, mark {mark_ms} ms"
    );
}

/// NEGATIVE CONTROL for the cached-plan case: without the settings. Printed.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_cached_plan_after_a_burst_without_the_settings_negative_control() {
    let (claim_ms, mark_ms) = cached_plan_after_a_burst(false).await;
    println!("[cached, NO settings] claim {claim_ms:.1} ms, mark-QUEUED {mark_ms:.1} ms");
}

// ─── The engine against a burst ─────────────────────────────────────────────

/// Publishes nothing; records first deliveries per group and fails the test
/// (afterwards) on a duplicate or an out-of-order delivery. Job ids increase
/// with the order inside a group, so "in order" is "ids increase".
struct OrderedPublisher {
    last: Mutex<HashMap<String, String>>,
    seen: Mutex<HashSet<String>>,
    out_of_order: Mutex<Vec<String>>,
    duplicates: Mutex<Vec<String>>,
    published: AtomicUsize,
    out: mpsc::UnboundedSender<String>,
}

#[async_trait::async_trait]
impl DispatchPublisher for OrderedPublisher {
    async fn publish(&self, items: Vec<PublishItem>) -> PublishOutcome {
        for item in items {
            if !self.seen.lock().unwrap().insert(item.job_id.clone()) {
                self.duplicates.lock().unwrap().push(item.job_id.clone());
                continue;
            }
            let group = item.group_id().to_string();
            let mut last = self.last.lock().unwrap();
            if last.get(&group).is_some_and(|prev| *prev > item.job_id) {
                self.out_of_order.lock().unwrap().push(item.job_id.clone());
            }
            last.insert(group, item.job_id.clone());
            drop(last);
            self.published.fetch_add(1, Ordering::SeqCst);
            let _ = self.out.send(item.job_id);
        }
        PublishOutcome::default()
    }
    fn describe(&self) -> String {
        "ordered".into()
    }
}

/// The class of test that would have caught the queue table's stall: realistic
/// width on a real database in the realistic statistics state (the active
/// partition analysed while no job was PENDING, then a burst of 200,000). The
/// real engine runs (10 lanes marking batches of 100, a poller claiming 500 at
/// a time, 1,000 jobs in flight) while callback-like workers move published
/// jobs PENDING -> PROCESSING -> COMPLETED on the platform's pool. No statement
/// may run longer than 250 ms (sampled from `pg_stat_activity` every 10 ms),
/// throughput may not collapse (a sargable status guard on the by-key
/// statements kept every single statement under 250 ms in Go and still lost
/// 80% of the throughput), and no job may be lost, published twice or
/// published out of order.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_engine_keeps_up_with_a_burst_without_a_slow_statement() {
    const TARGET: usize = 40_000;
    let (_db, url): (TestDb, String) = start_db("conc").await;
    let platform = create_pool(&url).await.unwrap();
    run_migrations(&platform, MigrationProfile::Production)
        .await
        .unwrap();
    prepare(
        &platform,
        State::ActiveAnalysedBeforeTheOrderedBurst,
        200_000,
    )
    .await;
    let spool = create_scheduler_pool(&url, 14).await.unwrap();

    let (tx, rx) = mpsc::unbounded_channel::<String>();
    let publisher = Arc::new(OrderedPublisher {
        last: Default::default(),
        seen: Default::default(),
        out_of_order: Default::default(),
        duplicates: Default::default(),
        published: Default::default(),
        out: tx,
    });
    let config = SchedulerConfig::default();
    let scheduler = DispatchScheduler::new(
        config,
        spool.clone(),
        publisher.clone(),
        DispatchAuthService::from_app_key("conc-test-app-key").unwrap(),
        Arc::new(PoolCodeResolver::new(
            spool.clone(),
            Duration::from_secs(60),
        )),
    );

    // The slowest statement seen, sampled.
    let slowest: Arc<Mutex<(f64, String)>> = Arc::new(Mutex::new((0.0, String::new())));
    let stop_sampling = CancellationToken::new();
    let sampler = {
        let monitor = platform.clone();
        let slowest = slowest.clone();
        let stop = stop_sampling.clone();
        tokio::spawn(async move {
            while !stop.is_cancelled() {
                let row: Option<(f64, String)> = sqlx::query_as(
                    "SELECT (EXTRACT(EPOCH FROM now() - query_start) * 1000)::float8, query \
                       FROM pg_stat_activity \
                      WHERE datname = current_database() AND state = 'active' \
                        AND backend_type = 'client backend' AND pid <> pg_backend_pid() \
                      ORDER BY query_start LIMIT 1",
                )
                .fetch_optional(&monitor)
                .await
                .ok()
                .flatten();
                if let Some((age, query)) = row {
                    let mut s = slowest.lock().unwrap();
                    if age > s.0 {
                        *s = (age, query);
                    }
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
    };

    // The callback side: two workers on the platform's pool.
    let rx = Arc::new(AsyncMutex::new(rx));
    let completed = Arc::new(AtomicUsize::new(0));
    let stop_callbacks = CancellationToken::new();
    let mut callbacks = Vec::new();
    for _ in 0..2 {
        let (rx, pool, completed, stop) = (
            rx.clone(),
            platform.clone(),
            completed.clone(),
            stop_callbacks.clone(),
        );
        callbacks.push(tokio::spawn(async move {
            loop {
                let id = tokio::select! {
                    () = stop.cancelled() => break,
                    id = async { rx.lock().await.recv().await } => match id { Some(id) => id, None => break },
                };
                let created: Option<chrono::DateTime<Utc>> =
                    sqlx::query_scalar("SELECT created_at FROM msg_dispatch_jobs WHERE id = $1")
                        .bind(&id)
                        .fetch_optional(&pool)
                        .await
                        .unwrap_or(None);
                let Some(created) = created else { continue };
                if lifecycle::claim_for_delivery(&pool, &id, created)
                    .await
                    .unwrap_or(false)
                    && lifecycle::complete(&pool, &id, created, 1)
                        .await
                        .unwrap_or(false)
                {
                    completed.fetch_add(1, Ordering::SeqCst);
                }
            }
        }));
    }

    let cancel = CancellationToken::new();
    let run = scheduler.poller().run(
        Duration::from_millis(100),
        Arc::new(|| true),
        cancel.clone(),
    );
    let started = Instant::now();
    let watch = async {
        while publisher.published.load(Ordering::SeqCst) < TARGET
            && started.elapsed() < Duration::from_secs(180)
        {
            sleep(Duration::from_millis(50)).await;
        }
        cancel.cancel();
    };
    tokio::join!(run, watch);
    // Let the callbacks drain what was published.
    sleep(Duration::from_secs(2)).await;
    stop_callbacks.cancel();
    for c in callbacks {
        let _ = c.await;
    }
    stop_sampling.cancel();
    let _ = sampler.await;
    let elapsed = started.elapsed();

    let published = publisher.published.load(Ordering::SeqCst);
    let rate = published as f64 / elapsed.as_secs_f64();
    let (max_ms, max_sql) = slowest.lock().unwrap().clone();
    println!(
        "CONCURRENCY: {published} jobs published in {elapsed:.1?} ({rate:.0} jobs/s), {} completed by the callbacks; slowest sampled statement {max_ms:.0} ms: {:.140}",
        completed.load(Ordering::SeqCst),
        max_sql.replace('\n', " ")
    );
    assert!(
        published >= TARGET,
        "published only {published} in {elapsed:?}"
    );
    assert!(rate > 6_000.0, "throughput collapsed: {rate:.0} jobs/s");
    assert!(max_ms < 250.0, "a statement ran {max_ms:.0} ms: {max_sql}");
    assert!(
        publisher.out_of_order.lock().unwrap().is_empty(),
        "published out of order: {:?}",
        publisher
            .out_of_order
            .lock()
            .unwrap()
            .iter()
            .take(5)
            .collect::<Vec<_>>()
    );
    assert!(
        publisher.duplicates.lock().unwrap().is_empty(),
        "published twice"
    );
    let ids: Vec<String> = publisher.seen.lock().unwrap().iter().cloned().collect();
    let pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM msg_dispatch_jobs WHERE id = ANY($1) AND status = 'PENDING'",
    )
    .bind(&ids)
    .fetch_one(&platform)
    .await
    .unwrap();
    assert_eq!(
        pending, 0,
        "a published job was left PENDING (a lost mark-QUEUED)"
    );
}
