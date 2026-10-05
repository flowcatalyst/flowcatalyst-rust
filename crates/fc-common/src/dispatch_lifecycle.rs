//! The dispatch-job lifecycle: the ONLY production code that inserts into
//! `msg_dispatch_jobs` or writes its `status`.
//!
//! Every status change of a dispatch job goes through a named operation in
//! this file. Each operation names the statuses it may move a job FROM (in
//! the SQL `WHERE`, not in application memory), stamps `updated_at`, and,
//! when the guard matches no row, is counted and logged as a refused
//! transition rather than failing. A late callback therefore never
//! overwrites or resurrects a settled job.
//!
//! # Where it lives
//!
//! `fc-stream`'s event fan-out (stream pool, its own transaction) and
//! `fc-platform-messaging` (API ingest, delivery callback, scheduler, stale
//! recovery, reaper, operator actions) both write jobs, and `fc-stream`
//! does not depend on the messaging crate (the standalone router links
//! `fc-stream`). `fc-common` is the lowest crate both already see, so the
//! lifecycle lives here behind the `dispatch-lifecycle` feature (which
//! pulls in `sqlx` and `metrics`; the default build stays free of both).
//! `fc_platform_messaging::dispatch_job::lifecycle` re-exports it.
//!
//! # Shape
//!
//! Every operation takes any sqlx executor (a pool, or `&mut *tx` inside a
//! transaction the caller owns) and issues one statement.
//!
//! Two private primitives are the only places a job's status is written
//! with respect to PENDING:
//!
//! * [`enter_pending`]: the single place a job becomes (or is refreshed as)
//!   PENDING: retry, deferral, hold, settled-return, reaper sweep, stale
//!   recovery, operator requeue. (Creation is the insert pair
//!   [`create_batch`] / [`create_fanned_out`]: an INSERT has no selector.)
//! * [`leave_pending`]: the single place a job stops being PENDING:
//!   scheduler mark-QUEUED, callback claim-for-delivery, and the terminal
//!   outcome writes, which may find the job PENDING.
//!
//! Both `RETURNING` the columns of [`Transitioned`]. Transitions with no
//! PENDING on either side ([`reclaim_stale_delivery`], the operator's cancel
//! and complete of a FAILED job) are also operations here but do not use the
//! primitives.
//!
//! [`TRANSITIONS`] states the whole lifecycle in one place; the tests at the
//! bottom pin it.
//!
//! # One table
//!
//! Every statement here touches `msg_dispatch_jobs` and nothing else. (A
//! queue table, `msg_dispatch_queue`, was tried and retired by migration 066:
//! a small table that swings between empty and very full is the planner's
//! worst case; every statement that joined it was planned as a scan when its
//! statistics said "empty".) The scheduler claims PENDING jobs straight from
//! the job table ([`claim`]) and keeps what it is publishing out of the next
//! claim with an in-memory in-flight set.
//!
//! # Status guards and the access path
//!
//! A statement that names its rows by primary key (by id, by `(id,
//! created_at)`, by the scheduler's claimed `(id, created_at, version)`)
//! writes its status guard as `j.status || '' = 'X'`: a sargable `status =
//! 'PENDING'` lets the planner walk the status index instead of the primary
//! key, and when the statistics say PENDING is absent (the active partition
//! analysed before a burst) it believes that costs one row and reads every
//! PENDING job (6 s at 200,000). Statements that SELECT BY status (the claim,
//! the stale sweeps, the reaper, the hold-back) keep the plain sargable form.

use chrono::{DateTime, Utc};
use sqlx::{Error, Executor, FromRow, Postgres, QueryBuilder};
use std::collections::HashMap;
use tracing::{debug, warn};

use crate::DispatchStatus;

/// The live statuses: a job in one of these is still in play.
const LIVE: &[DispatchStatus] = &[
    DispatchStatus::Pending,
    DispatchStatus::Queued,
    DispatchStatus::Processing,
];

// ─── The lifecycle, stated once ─────────────────────────────────────────────

/// One named transition: the statuses it may move a job from (an empty
/// slice means "any status") and the status it moves it to.
#[derive(Debug, Clone, Copy)]
pub struct Transition {
    /// The `transition` label on the refusal counter.
    pub name: &'static str,
    /// Allowed "from" statuses; empty = any (operator requeue only).
    pub from: &'static [DispatchStatus],
    pub to: DispatchStatus,
}

impl Transition {
    /// The status a job in `status` ends in under this transition, or
    /// `None` when the transition refuses it.
    pub fn apply(&self, status: DispatchStatus) -> Option<DispatchStatus> {
        (self.from.is_empty() || self.from.contains(&status)).then_some(self.to)
    }

    /// `status IN (...)` with literal statuses. `opaque` writes `j.status ||
    /// ''` so the guard cannot be an index condition (for a statement that
    /// already pins its rows by primary key: see the module docs).
    fn guard_sql(&self, opaque: bool) -> Option<String> {
        let col = if opaque { "j.status || ''" } else { "j.status" };
        match self.from {
            [] => None,
            [one] => Some(format!("{col} = '{}'", one.as_str())),
            many => {
                let list: Vec<String> = many.iter().map(|s| format!("'{}'", s.as_str())).collect();
                Some(format!("{col} IN ({})", list.join(", ")))
            }
        }
    }
}

/// A callback outcome that is not terminal: a retryable failure.
pub const RETRY: Transition = Transition {
    name: "retry",
    from: LIVE,
    to: DispatchStatus::Pending,
};
/// A subscriber's cooperative deferral (`ack:false`, 429).
pub const DEFER: Transition = Transition {
    name: "defer",
    from: LIVE,
    to: DispatchStatus::Pending,
};
/// A job held behind its group at delivery time.
pub const HOLD: Transition = Transition {
    name: "hold",
    from: LIVE,
    to: DispatchStatus::Pending,
};
/// The router's settled-message hook returning a job to PENDING.
pub const SETTLE_RETURN: Transition = Transition {
    name: "settle_return",
    from: &[DispatchStatus::Queued, DispatchStatus::Processing],
    to: DispatchStatus::Pending,
};
/// The stranded-sibling reaper.
pub const REAP: Transition = Transition {
    name: "reap",
    from: &[DispatchStatus::Queued, DispatchStatus::Processing],
    to: DispatchStatus::Pending,
};
/// Stale-recovery sweep of long-QUEUED jobs.
pub const STALE_QUEUED: Transition = Transition {
    name: "stale_queued",
    from: &[DispatchStatus::Queued],
    to: DispatchStatus::Pending,
};
/// Stale-recovery sweep of long-PROCESSING jobs.
pub const STALE_PROCESSING: Transition = Transition {
    name: "stale_processing",
    from: &[DispatchStatus::Processing],
    to: DispatchStatus::Pending,
};
/// Operator requeue: the one transition allowed from any status.
pub const REQUEUE: Transition = Transition {
    name: "requeue",
    from: &[],
    to: DispatchStatus::Pending,
};
/// The scheduler marks a published job QUEUED.
pub const MARK_QUEUED: Transition = Transition {
    name: "mark_queued",
    from: &[DispatchStatus::Pending],
    to: DispatchStatus::Queued,
};
/// The delivery callback claims a job.
pub const CLAIM: Transition = Transition {
    name: "claim",
    from: &[DispatchStatus::Pending, DispatchStatus::Queued],
    to: DispatchStatus::Processing,
};
/// A successful delivery.
pub const COMPLETE: Transition = Transition {
    name: "complete",
    from: LIVE,
    to: DispatchStatus::Completed,
};
/// A terminal delivery failure.
pub const FAIL: Transition = Transition {
    name: "fail",
    from: LIVE,
    to: DispatchStatus::Failed,
};
/// A dead delivery's lease taken over.
pub const RECLAIM: Transition = Transition {
    name: "reclaim",
    from: &[DispatchStatus::Processing],
    to: DispatchStatus::Processing,
};
/// An operator cancels a FAILED job.
pub const OPERATOR_CANCEL: Transition = Transition {
    name: "operator_cancel",
    from: &[DispatchStatus::Failed],
    to: DispatchStatus::Cancelled,
};
/// An operator completes a FAILED job by hand.
pub const OPERATOR_COMPLETE: Transition = Transition {
    name: "operator_complete",
    from: &[DispatchStatus::Failed],
    to: DispatchStatus::Completed,
};

/// Every transition, for the table-driven tests and the documentation.
pub const TRANSITIONS: &[Transition] = &[
    RETRY,
    DEFER,
    HOLD,
    SETTLE_RETURN,
    REAP,
    STALE_QUEUED,
    STALE_PROCESSING,
    REQUEUE,
    MARK_QUEUED,
    CLAIM,
    COMPLETE,
    FAIL,
    RECLAIM,
    OPERATOR_CANCEL,
    OPERATOR_COMPLETE,
];

// ─── Results ────────────────────────────────────────────────────────────────

/// What every transition returns for each row it moved: the columns a queue
/// table would need.
#[derive(Debug, Clone, FromRow)]
pub struct Transitioned {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub message_group: Option<String>,
    pub sequence: i32,
    pub scheduled_for: Option<DateTime<Utc>>,
    pub subscription_id: Option<String>,
    pub dispatch_pool_id: Option<String>,
    pub client_id: Option<String>,
    pub mode: String,
    pub queue: Option<String>,
    pub updated_at: DateTime<Utc>,
}

const RETURNING: &str = " RETURNING j.id, j.created_at, j.message_group, j.sequence, \
    j.scheduled_for, j.subscription_id, j.dispatch_pool_id, j.client_id, j.mode, j.queue, \
    j.updated_at";

// ─── Selectors and changes (private) ────────────────────────────────────────

/// Which rows a transition addresses.
enum Selector<'a> {
    /// One job by `(id, created_at)`: the partition key prunes to one
    /// partition.
    One {
        id: &'a str,
        created_at: DateTime<Utc>,
    },
    /// A list of ids (no `created_at` known: scans by id).
    Ids(&'a [String]),
    /// A list of `(id, created_at)`.
    Keys(&'a [(String, DateTime<Utc>)]),
    /// A list of `(id, created_at, updated_at as read)`: only the row
    /// version that was read matches (the scheduler's optimistic check).
    Versioned(&'a [(String, DateTime<Utc>, DateTime<Utc>)]),
    /// Sweep: every row whose `updated_at` is before `before`.
    StaleSince { before: DateTime<Utc> },
    /// Sweep: the siblings of a FAILED/ERROR BLOCK_ON_ERROR head that are
    /// stranded QUEUED/PROCESSING. A PROCESSING row updated since
    /// `live_before` is presumed in flight and left alone.
    StrandedSiblings { live_before: DateTime<Utc> },
}

impl Selector<'_> {
    /// How many rows the caller expects to move, for refusal counting;
    /// `None` for a sweep (matching nothing is the normal case there).
    fn expected(&self) -> Option<usize> {
        match self {
            Selector::One { .. } => Some(1),
            Selector::Ids(v) => Some(v.len()),
            Selector::Keys(v) => Some(v.len()),
            Selector::Versioned(v) => Some(v.len()),
            Selector::StaleSince { .. } | Selector::StrandedSiblings { .. } => None,
        }
    }

    /// Whether the selector names its rows by primary key, so the status
    /// guard is written opaque (see the module docs).
    fn pins_by_key(&self) -> bool {
        matches!(
            self,
            Selector::One { .. } | Selector::Ids(_) | Selector::Keys(_) | Selector::Versioned(_)
        )
    }

    /// The leading `WITH` common table expressions the selector needs (the
    /// stranded-sibling set; the scheduler's claimed `(id, created_at,
    /// version)` batch). Returns whether a `WITH` was started.
    fn push_ctes(&self, qb: &mut QueryBuilder<'_, Postgres>) -> bool {
        match self {
            Selector::StrandedSiblings { live_before } => {
                qb.push(
                    "WITH stranded AS ( \
                         SELECT s.id, s.created_at \
                           FROM msg_dispatch_jobs s \
                           JOIN msg_dispatch_jobs h \
                             ON h.message_group = s.message_group \
                            AND h.status IN ('FAILED', 'ERROR') \
                            AND (h.sequence, h.created_at, h.id) < (s.sequence, s.created_at, s.id) \
                          WHERE s.mode = 'BLOCK_ON_ERROR' \
                            AND s.message_group IS NOT NULL \
                            AND s.status IN ('QUEUED', 'PROCESSING') \
                            AND (s.status <> 'PROCESSING' OR s.updated_at < ",
                )
                .push_bind(*live_before)
                .push(") )");
                true
            }
            Selector::Versioned(keys) => {
                // Each (id, created_at) is looked up by primary key alone in a
                // LATERAL sub-query (OFFSET 0 keeps the planner from flattening
                // it into a join it might run as a scan; a status test inside
                // it would let the planner walk the status index); the version
                // and status are checked on what it returns.
                let ids: Vec<String> = keys.iter().map(|k| k.0.clone()).collect();
                let created: Vec<DateTime<Utc>> = keys.iter().map(|k| k.1).collect();
                let updated: Vec<DateTime<Utc>> = keys.iter().map(|k| k.2).collect();
                qb.push("WITH pk AS ( SELECT p.id, p.created_at, c.v FROM UNNEST(")
                    .push_bind(ids)
                    .push("::varchar[], ")
                    .push_bind(created)
                    .push("::timestamptz[], ")
                    .push_bind(updated)
                    .push(
                        "::timestamptz[]) AS c(id, created_at, v) \
                         CROSS JOIN LATERAL ( SELECT id, created_at, updated_at, status \
                         FROM msg_dispatch_jobs WHERE id = c.id AND created_at = c.created_at \
                         OFFSET 0 ) p \
                         WHERE p.updated_at = c.v AND p.status = 'PENDING' )",
                    );
                true
            }
            _ => false,
        }
    }

    fn push_from(&self, qb: &mut QueryBuilder<'_, Postgres>) {
        match self {
            Selector::Keys(keys) => {
                let ids: Vec<String> = keys.iter().map(|k| k.0.clone()).collect();
                let created: Vec<DateTime<Utc>> = keys.iter().map(|k| k.1).collect();
                qb.push(" FROM UNNEST(")
                    .push_bind(ids)
                    .push("::varchar[], ")
                    .push_bind(created)
                    .push("::timestamptz[]) AS t(id, created_at)");
            }
            Selector::Versioned(_) => {
                qb.push(" FROM pk");
            }
            Selector::StrandedSiblings { .. } => {
                qb.push(" FROM stranded st");
            }
            Selector::One { .. } | Selector::Ids(_) | Selector::StaleSince { .. } => {}
        }
    }

    fn push_where(&self, qb: &mut QueryBuilder<'_, Postgres>) {
        match self {
            Selector::One { id, created_at } => {
                qb.push(" WHERE j.id = ")
                    .push_bind(id.to_string())
                    .push(" AND j.created_at = ")
                    .push_bind(*created_at);
            }
            Selector::Ids(ids) => {
                qb.push(" WHERE j.id = ANY(")
                    .push_bind(ids.to_vec())
                    .push(")");
            }
            Selector::Keys(_) => {
                qb.push(" WHERE j.id = t.id AND j.created_at = t.created_at");
            }
            Selector::Versioned(_) => {
                // The version is checked again here, on the row the UPDATE
                // finally locks: a re-enter that commits while the statement
                // waits for the row lock moves `updated_at`, and the lateral
                // sub-query (read from the statement's snapshot) cannot see it.
                // It is a correlated sub-query, not a join clause: written as
                // `j.updated_at = pk.v` the planner, in the no-statistics
                // states, joined the jobs table to `pk` with a hash join over
                // a seq scan (3.5 s) instead of 500 primary-key probes.
                qb.push(
                    " WHERE j.id = pk.id AND j.created_at = pk.created_at \
                     AND j.updated_at = ( SELECT k.v FROM pk k \
                     WHERE k.id = j.id AND k.created_at = j.created_at LIMIT 1 )",
                );
            }
            Selector::StaleSince { before } => {
                qb.push(" WHERE j.updated_at < ").push_bind(*before);
            }
            Selector::StrandedSiblings { live_before } => {
                // The age test is repeated here, on the row the UPDATE finally
                // locks: the CTE decided it from the statement's snapshot, and
                // a delivery callback that claims the sibling (PROCESSING, a
                // fresh `updated_at`) while the UPDATE waits for the row lock
                // must not be reset to PENDING.
                qb.push(
                    " WHERE j.id = st.id AND j.created_at = st.created_at \
                     AND (j.status <> 'PROCESSING' OR j.updated_at < ",
                )
                .push_bind(*live_before)
                .push(")");
            }
        }
    }
}

/// A column a transition sets besides `status` and `updated_at`.
enum Change<'a> {
    ScheduledFor(Option<DateTime<Utc>>),
    LastError(Option<&'a str>),
    ClearQueuedAt,
    StampQueuedAt,
    StampLastAttempt,
    StampCompletedAt,
    DurationMillis(i64),
    BumpAttemptCount,
    ResetAttemptCount,
    ClearCompletion,
}

impl Change<'_> {
    fn push(&self, qb: &mut QueryBuilder<'_, Postgres>) {
        match self {
            Change::ScheduledFor(v) => {
                qb.push("scheduled_for = ").push_bind(*v);
            }
            Change::LastError(v) => {
                qb.push("last_error = ").push_bind(v.map(str::to_string));
            }
            Change::ClearQueuedAt => {
                qb.push("queued_at = NULL");
            }
            Change::StampQueuedAt => {
                qb.push("queued_at = NOW()");
            }
            Change::StampLastAttempt => {
                qb.push("last_attempt_at = NOW()");
            }
            Change::StampCompletedAt => {
                qb.push("completed_at = NOW()");
            }
            Change::DurationMillis(v) => {
                qb.push("duration_millis = ").push_bind(*v);
            }
            Change::BumpAttemptCount => {
                qb.push("attempt_count = attempt_count + 1");
            }
            Change::ResetAttemptCount => {
                qb.push("attempt_count = 0");
            }
            Change::ClearCompletion => {
                qb.push("completed_at = NULL, duration_millis = NULL");
            }
        }
    }
}

// ─── The statement builder and the two primitives (private) ─────────────────

/// An extra predicate pushed after the status guard (only
/// [`reclaim_stale_delivery`] uses one).
enum Extra {
    None,
    /// The current claim was made before this instant.
    ClaimedBefore(DateTime<Utc>),
}

/// Builds ONE statement for `t`: the `UPDATE msg_dispatch_jobs` (status to
/// `t.to`, the guard on `t.from`, `updated_at = NOW()`, the selector) with its
/// `RETURNING` of [`Transitioned`].
fn build_update<'q>(
    t: &Transition,
    sel: &Selector<'_>,
    changes: &[Change<'_>],
    extra: &Extra,
) -> QueryBuilder<'q, Postgres> {
    build_update_after(t, sel, changes, extra, "")
}

/// [`build_update`] with `prefix` (`EXPLAIN (...) `, in the plan tests) in
/// front of the statement.
fn build_update_after<'q>(
    t: &Transition,
    sel: &Selector<'_>,
    changes: &[Change<'_>],
    extra: &Extra,
    prefix: &str,
) -> QueryBuilder<'q, Postgres> {
    let mut qb: QueryBuilder<'q, Postgres> = QueryBuilder::new(prefix);
    if sel.push_ctes(&mut qb) {
        qb.push(" ");
    }
    qb.push("UPDATE msg_dispatch_jobs AS j SET status = '")
        .push(t.to.as_str())
        .push("'");
    for c in changes {
        qb.push(", ");
        c.push(&mut qb);
    }
    qb.push(", updated_at = NOW()");
    sel.push_from(&mut qb);
    sel.push_where(&mut qb);
    if let Some(g) = t.guard_sql(sel.pins_by_key()) {
        qb.push(" AND ").push(g);
    }
    if let Extra::ClaimedBefore(at) = extra {
        qb.push(" AND COALESCE(j.last_attempt_at, j.updated_at) < ")
            .push_bind(*at);
    }
    qb.push(RETURNING);
    qb
}

/// Runs the statement [`build_update`] builds. The two primitives and the
/// two non-PENDING transitions are the callers.
async fn run_transition<'e, E>(
    ex: E,
    t: &Transition,
    sel: &Selector<'_>,
    changes: &[Change<'_>],
    extra: &Extra,
) -> Result<Vec<Transitioned>, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    // Nothing to address: no statement, nothing refused.
    if matches!(sel.expected(), Some(0)) {
        return Ok(Vec::new());
    }
    let mut qb = build_update(t, sel, changes, extra);
    let rows: Vec<Transitioned> = qb.build_query_as().fetch_all(ex).await?;
    record_outcome(t, sel.expected(), rows.len());
    Ok(rows)
}

/// The single place a job becomes (or is refreshed as) PENDING.
async fn enter_pending<'e, E>(
    ex: E,
    t: &Transition,
    sel: Selector<'_>,
    changes: &[Change<'_>],
) -> Result<Vec<Transitioned>, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    debug_assert_eq!(t.to, DispatchStatus::Pending);
    run_transition(ex, t, &sel, changes, &Extra::None).await
}

/// The single place a job stops being PENDING.
async fn leave_pending<'e, E>(
    ex: E,
    t: &Transition,
    sel: Selector<'_>,
    changes: &[Change<'_>],
) -> Result<Vec<Transitioned>, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    debug_assert_ne!(t.to, DispatchStatus::Pending);
    debug_assert!(t.from.is_empty() || t.from.contains(&DispatchStatus::Pending));
    run_transition(ex, t, &sel, changes, &Extra::None).await
}

/// Counts and logs a transition that moved fewer rows than addressed. Not
/// an error: callers that ignore the row count keep working.
fn record_outcome(t: &Transition, expected: Option<usize>, moved: usize) {
    let Some(expected) = expected else {
        return;
    };
    let refused = expected.saturating_sub(moved);
    if refused == 0 {
        return;
    }
    metrics::counter!("dispatch_job.transition_refused_total", "transition" => t.name)
        .increment(refused as u64);
    if expected == 1 {
        debug!(
            transition = t.name,
            "dispatch job transition refused: no row in an allowed status"
        );
    } else {
        debug!(
            transition = t.name,
            expected,
            moved,
            refused,
            "dispatch job transitions refused: rows not in an allowed status"
        );
    }
}

// ─── Creation ───────────────────────────────────────────────────────────────

/// A job to insert in full (API ingest, SDK batch). It has no status:
/// [`create_batch`] always inserts PENDING (and its queue row). A test that
/// needs a job in another status moves it there after creating it.
#[derive(Debug, Clone)]
pub struct NewJob {
    pub id: String,
    pub external_id: Option<String>,
    pub source: Option<String>,
    pub kind: String,
    pub code: String,
    pub subject: Option<String>,
    pub event_id: Option<String>,
    pub correlation_id: Option<String>,
    pub metadata: serde_json::Value,
    pub target_url: String,
    pub protocol: String,
    pub payload: Option<String>,
    pub payload_content_type: String,
    pub data_only: bool,
    pub service_account_id: Option<String>,
    pub client_id: Option<String>,
    pub subscription_id: Option<String>,
    pub mode: String,
    pub dispatch_pool_id: Option<String>,
    pub message_group: Option<String>,
    pub sequence: i32,
    pub timeout_seconds: i32,
    pub schema_id: Option<String>,
    pub max_retries: i32,
    pub retry_strategy: String,
    pub scheduled_for: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub attempt_count: i32,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub duration_millis: Option<i64>,
    pub last_error: Option<String>,
    pub idempotency_key: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub descriptor: Option<String>,
    pub queue: Option<String>,
}

/// The 37-column insert of [`create_batch`]: every job PENDING.
fn create_batch_sql() -> String {
    String::from(
        r#"INSERT INTO msg_dispatch_jobs
            (id, external_id, source, kind, code, subject, event_id, correlation_id,
             metadata, target_url, protocol, payload, payload_content_type, data_only,
             service_account_id, client_id, subscription_id, mode, dispatch_pool_id,
             message_group, sequence, timeout_seconds, schema_id, max_retries,
             retry_strategy, scheduled_for, expires_at, attempt_count, last_attempt_at,
             completed_at, duration_millis, last_error, idempotency_key, created_at, updated_at,
             descriptor, queue, status)
        SELECT u.*, 'PENDING' FROM UNNEST(
            $1::varchar[], $2::varchar[], $3::varchar[], $4::varchar[], $5::varchar[],
            $6::varchar[], $7::varchar[], $8::varchar[], $9::jsonb[], $10::varchar[],
            $11::varchar[], $12::text[], $13::varchar[], $14::bool[],
            $15::varchar[], $16::varchar[], $17::varchar[], $18::varchar[], $19::varchar[],
            $20::varchar[], $21::int4[], $22::int4[], $23::varchar[],
            $24::int4[], $25::varchar[], $26::timestamptz[], $27::timestamptz[],
            $28::int4[], $29::timestamptz[], $30::timestamptz[], $31::int8[],
            $32::varchar[], $33::varchar[], $34::timestamptz[], $35::timestamptz[],
            $36::varchar[], $37::varchar[]
        ) AS u"#,
    )
}

/// Insert jobs in full, always PENDING, one `UNNEST` statement. A single job
/// is a batch of one. Run it inside the caller's transaction when other writes must be
/// atomic with it.
pub async fn create_batch<'e, E>(ex: E, jobs: &[NewJob]) -> Result<(), Error>
where
    E: Executor<'e, Database = Postgres>,
{
    if jobs.is_empty() {
        return Ok(());
    }
    fn col<'a, T>(jobs: &'a [NewJob], f: impl Fn(&'a NewJob) -> T) -> Vec<T> {
        jobs.iter().map(f).collect()
    }
    sqlx::query(&create_batch_sql())
        .bind(col(jobs, |j| j.id.as_str()))
        .bind(col(jobs, |j| j.external_id.as_deref()))
        .bind(col(jobs, |j| j.source.as_deref()))
        .bind(col(jobs, |j| j.kind.as_str()))
        .bind(col(jobs, |j| j.code.as_str()))
        .bind(col(jobs, |j| j.subject.as_deref()))
        .bind(col(jobs, |j| j.event_id.as_deref()))
        .bind(col(jobs, |j| j.correlation_id.as_deref()))
        .bind(col(jobs, |j| j.metadata.clone()))
        .bind(col(jobs, |j| j.target_url.as_str()))
        .bind(col(jobs, |j| j.protocol.as_str()))
        .bind(col(jobs, |j| j.payload.as_deref()))
        .bind(col(jobs, |j| Some(j.payload_content_type.as_str())))
        .bind(col(jobs, |j| j.data_only))
        .bind(col(jobs, |j| j.service_account_id.as_deref()))
        .bind(col(jobs, |j| j.client_id.as_deref()))
        .bind(col(jobs, |j| j.subscription_id.as_deref()))
        .bind(col(jobs, |j| j.mode.as_str()))
        .bind(col(jobs, |j| j.dispatch_pool_id.as_deref()))
        .bind(col(jobs, |j| j.message_group.as_deref()))
        .bind(col(jobs, |j| j.sequence))
        .bind(col(jobs, |j| j.timeout_seconds))
        .bind(col(jobs, |j| j.schema_id.as_deref()))
        .bind(col(jobs, |j| j.max_retries))
        .bind(col(jobs, |j| j.retry_strategy.as_str()))
        .bind(col(jobs, |j| j.scheduled_for))
        .bind(col(jobs, |j| j.expires_at))
        .bind(col(jobs, |j| j.attempt_count))
        .bind(col(jobs, |j| j.last_attempt_at))
        .bind(col(jobs, |j| j.completed_at))
        .bind(col(jobs, |j| j.duration_millis))
        .bind(col(jobs, |j| j.last_error.as_deref()))
        .bind(col(jobs, |j| j.idempotency_key.as_deref()))
        .bind(col(jobs, |j| j.created_at))
        .bind(col(jobs, |j| j.updated_at))
        .bind(col(jobs, |j| j.descriptor.as_deref()))
        .bind(col(jobs, |j| j.queue.as_deref()))
        .execute(ex)
        .await?;
    Ok(())
}

/// A job raised by the event fan-out. Carries only the columns fan-out sets;
/// every other column (kind='EVENT', retry_strategy='exponential',
/// external_id=NULL, ...) takes the table default. Always inserted PENDING.
#[derive(Debug, Clone)]
pub struct FanOutJob {
    pub id: String,
    pub code: String,
    pub source: String,
    pub subject: Option<String>,
    pub event_id: String,
    pub correlation_id: Option<String>,
    pub target_url: String,
    pub protocol: &'static str,
    pub payload: String,
    pub data_only: bool,
    pub service_account_id: Option<String>,
    pub client_id: Option<String>,
    pub subscription_id: String,
    pub queue: Option<String>,
    /// The raising subscription's name.
    pub descriptor: Option<String>,
    /// The raising event's `context_data`, verbatim; `None` takes `[]`.
    pub metadata: Option<serde_json::Value>,
    pub mode: &'static str,
    pub dispatch_pool_id: Option<String>,
    pub message_group: Option<String>,
    pub sequence: i32,
    pub timeout_seconds: i32,
    pub max_retries: i32,
    /// `{event.id}:{subscription.id}`.
    pub idempotency_key: String,
    /// Inherits the source event's `created_at` so the scheduler's order
    /// preserves source order and the job lands in the event's partition.
    pub created_at: DateTime<Utc>,
}

/// The fan-out insert of [`create_fanned_out`].
fn create_fanned_out_sql() -> String {
    String::from(
        r#"
        INSERT INTO msg_dispatch_jobs (
            id, code, source, subject, event_id, correlation_id,
            target_url, protocol, payload, data_only, service_account_id, client_id,
            subscription_id, mode, dispatch_pool_id, message_group,
            sequence, timeout_seconds, status, max_retries, idempotency_key,
            created_at, updated_at, queue, descriptor, metadata
        )
        SELECT
            u.id, u.code, u.source, u.subject, u.event_id, u.correlation_id,
            u.target_url, u.protocol, u.payload, u.data_only, u.service_account_id, u.client_id,
            u.subscription_id, u.mode, u.dispatch_pool_id, u.message_group,
            u.sequence, u.timeout_seconds, 'PENDING', u.max_retries, u.idempotency_key,
            u.created_at, u.created_at, u.queue, u.descriptor,
            -- An event without context data raises a job with no metadata.
            COALESCE(u.metadata, '[]'::jsonb)
        FROM UNNEST(
            $1::varchar[], $2::varchar[], $3::varchar[], $4::varchar[],
            $5::varchar[], $6::varchar[],
            $7::varchar[], $8::varchar[], $9::text[], $10::bool[], $11::varchar[], $12::varchar[],
            $13::varchar[], $14::varchar[], $15::varchar[], $16::varchar[],
            $17::int[], $18::int[], $19::int[], $20::varchar[],
            $21::timestamptz[], $22::varchar[], $23::varchar[], $24::jsonb[]
        ) AS u(
            id, code, source, subject, event_id, correlation_id,
            target_url, protocol, payload, data_only, service_account_id, client_id,
            subscription_id, mode, dispatch_pool_id, message_group,
            sequence, timeout_seconds, max_retries, idempotency_key,
            created_at, queue, descriptor, metadata
        )"#,
    )
}

/// Insert fan-out jobs as PENDING, one `UNNEST` statement, in the caller's
/// transaction (with the `fanned_out_at` stamp).
pub async fn create_fanned_out<'e, E>(ex: E, jobs: &[FanOutJob]) -> Result<(), Error>
where
    E: Executor<'e, Database = Postgres>,
{
    if jobs.is_empty() {
        return Ok(());
    }
    fn col<'a, T>(jobs: &'a [FanOutJob], f: impl Fn(&'a FanOutJob) -> T) -> Vec<T> {
        jobs.iter().map(f).collect()
    }
    sqlx::query(&create_fanned_out_sql())
        .bind(col(jobs, |j| j.id.as_str()))
        .bind(col(jobs, |j| j.code.as_str()))
        .bind(col(jobs, |j| j.source.as_str()))
        .bind(col(jobs, |j| j.subject.as_deref()))
        .bind(col(jobs, |j| Some(j.event_id.as_str())))
        .bind(col(jobs, |j| j.correlation_id.as_deref()))
        .bind(col(jobs, |j| j.target_url.as_str()))
        .bind(col(jobs, |j| j.protocol))
        .bind(col(jobs, |j| j.payload.as_str()))
        .bind(col(jobs, |j| j.data_only))
        .bind(col(jobs, |j| j.service_account_id.as_deref()))
        .bind(col(jobs, |j| j.client_id.as_deref()))
        .bind(col(jobs, |j| j.subscription_id.as_str()))
        .bind(col(jobs, |j| j.mode))
        .bind(col(jobs, |j| j.dispatch_pool_id.as_deref()))
        .bind(col(jobs, |j| j.message_group.as_deref()))
        .bind(col(jobs, |j| j.sequence))
        .bind(col(jobs, |j| j.timeout_seconds))
        .bind(col(jobs, |j| j.max_retries))
        .bind(col(jobs, |j| j.idempotency_key.as_str()))
        .bind(col(jobs, |j| j.created_at))
        .bind(col(jobs, |j| j.queue.as_deref()))
        .bind(col(jobs, |j| j.descriptor.as_deref()))
        .bind(col(jobs, |j| j.metadata.clone()))
        .execute(ex)
        .await?;
    Ok(())
}

// ─── Entering PENDING ───────────────────────────────────────────────────────

/// A retryable failure: back to PENDING at `scheduled_for`, spending one
/// attempt of the budget. From a live status only. `true` when the job moved.
pub async fn schedule_retry<'e, E>(
    ex: E,
    id: &str,
    created_at: DateTime<Utc>,
    scheduled_for: DateTime<Utc>,
    last_error: &str,
) -> Result<bool, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    let rows = enter_pending(
        ex,
        &RETRY,
        Selector::One { id, created_at },
        &[
            Change::BumpAttemptCount,
            Change::ScheduledFor(Some(scheduled_for)),
            Change::LastError(Some(last_error)),
            Change::StampLastAttempt,
            Change::ClearQueuedAt,
        ],
    )
    .await?;
    Ok(!rows.is_empty())
}

/// A subscriber's cooperative deferral (`ack:false`, 429): back to PENDING at
/// `scheduled_for` without spending the budget. From a live status only.
pub async fn defer<'e, E>(
    ex: E,
    id: &str,
    created_at: DateTime<Utc>,
    scheduled_for: DateTime<Utc>,
) -> Result<bool, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    reschedule(ex, &DEFER, id, created_at, scheduled_for).await
}

/// A job held behind its group at delivery time: back to PENDING at
/// `scheduled_for` without spending the budget. From a live status only.
pub async fn hold<'e, E>(
    ex: E,
    id: &str,
    created_at: DateTime<Utc>,
    scheduled_for: DateTime<Utc>,
) -> Result<bool, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    reschedule(ex, &HOLD, id, created_at, scheduled_for).await
}

async fn reschedule<'e, E>(
    ex: E,
    t: &Transition,
    id: &str,
    created_at: DateTime<Utc>,
    scheduled_for: DateTime<Utc>,
) -> Result<bool, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    let rows = enter_pending(
        ex,
        t,
        Selector::One { id, created_at },
        &[
            Change::ScheduledFor(Some(scheduled_for)),
            Change::ClearQueuedAt,
        ],
    )
    .await?;
    Ok(!rows.is_empty())
}

/// The router's settled-message hook: reset `ids` still QUEUED or PROCESSING
/// to PENDING, recording `reason`. Returns the rows reset. No `created_at`
/// is known, so this scans by id.
pub async fn return_settled<'e, E>(
    ex: E,
    ids: &[String],
    reason: &str,
) -> Result<Vec<Transitioned>, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    enter_pending(
        ex,
        &SETTLE_RETURN,
        Selector::Ids(ids),
        &[
            Change::ScheduledFor(None),
            Change::ClearQueuedAt,
            Change::LastError(Some(reason)),
        ],
    )
    .await
}

/// The reaper backstop: reset to PENDING every QUEUED/PROCESSING
/// BLOCK_ON_ERROR job whose group is headed by an earlier FAILED/ERROR job.
/// A PROCESSING row updated since `live_before` is presumed in flight.
pub async fn reap_stranded_siblings<'e, E>(
    ex: E,
    live_before: DateTime<Utc>,
    reason: &str,
) -> Result<Vec<Transitioned>, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    enter_pending(
        ex,
        &REAP,
        Selector::StrandedSiblings { live_before },
        &[
            Change::ScheduledFor(None),
            Change::ClearQueuedAt,
            Change::LastError(Some(reason)),
        ],
    )
    .await
}

/// Stale recovery: every job QUEUED since before `cutoff` (by `updated_at`)
/// goes back to PENDING.
pub async fn recover_stale_queued<'e, E>(
    ex: E,
    cutoff: DateTime<Utc>,
) -> Result<Vec<Transitioned>, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    enter_pending(
        ex,
        &STALE_QUEUED,
        Selector::StaleSince { before: cutoff },
        &[Change::ClearQueuedAt],
    )
    .await
}

/// Stale recovery: every job PROCESSING since before `cutoff` (by
/// `updated_at`) lost its outcome write and goes back to PENDING.
pub async fn recover_stale_processing<'e, E>(
    ex: E,
    cutoff: DateTime<Utc>,
    reason: &str,
) -> Result<Vec<Transitioned>, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    enter_pending(
        ex,
        &STALE_PROCESSING,
        Selector::StaleSince { before: cutoff },
        &[Change::ClearQueuedAt, Change::LastError(Some(reason))],
    )
    .await
}

/// Operator requeue: the jobs back to PENDING with a fresh attempt budget,
/// from ANY status. Run inside the use case's unit of work.
pub async fn requeue<'e, E>(
    ex: E,
    jobs: &[(String, DateTime<Utc>)],
) -> Result<Vec<Transitioned>, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    enter_pending(
        ex,
        &REQUEUE,
        Selector::Keys(jobs),
        &[
            Change::ScheduledFor(None),
            Change::ResetAttemptCount,
            Change::ClearCompletion,
            Change::LastError(None),
            Change::ClearQueuedAt,
        ],
    )
    .await
}

// ─── Leaving PENDING ────────────────────────────────────────────────────────

/// The scheduler marks exactly the published jobs QUEUED, and only the row
/// version it claimed: a job the callback moved on, or put back to PENDING
/// (every status write stamps `updated_at`) in the meantime, is not the
/// version claimed and is left alone. Returns the rows marked.
pub async fn mark_queued<'e, E>(
    ex: E,
    claimed: &[(String, DateTime<Utc>, DateTime<Utc>)],
) -> Result<Vec<Transitioned>, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    leave_pending(
        ex,
        &MARK_QUEUED,
        Selector::Versioned(claimed),
        &[Change::StampQueuedAt],
    )
    .await
}

/// Atomically claim a job for one delivery: PENDING/QUEUED to PROCESSING.
/// `false` means another delivery holds it (or it finished).
pub async fn claim_for_delivery<'e, E>(
    ex: E,
    id: &str,
    created_at: DateTime<Utc>,
) -> Result<bool, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    let rows = leave_pending(
        ex,
        &CLAIM,
        Selector::One { id, created_at },
        &[Change::StampLastAttempt],
    )
    .await?;
    Ok(!rows.is_empty())
}

/// Delivery succeeded: COMPLETED, stamping `completed_at` and the attempt's
/// duration. From a live status only: a settled job is never overwritten.
pub async fn complete<'e, E>(
    ex: E,
    id: &str,
    created_at: DateTime<Utc>,
    duration_millis: i64,
) -> Result<bool, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    let rows = leave_pending(
        ex,
        &COMPLETE,
        Selector::One { id, created_at },
        &[
            Change::StampCompletedAt,
            Change::DurationMillis(duration_millis),
        ],
    )
    .await?;
    Ok(!rows.is_empty())
}

/// Delivery failed terminally: FAILED, stamping `last_error`,
/// `completed_at` and the duration. From a live status only.
pub async fn fail<'e, E>(
    ex: E,
    id: &str,
    created_at: DateTime<Utc>,
    last_error: &str,
    duration_millis: i64,
) -> Result<bool, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    let rows = leave_pending(
        ex,
        &FAIL,
        Selector::One { id, created_at },
        &[
            Change::StampCompletedAt,
            Change::DurationMillis(duration_millis),
            Change::LastError(Some(last_error)),
        ],
    )
    .await?;
    Ok(!rows.is_empty())
}

// ─── Transitions with no PENDING on either side ─────────────────────────────

/// Take over a delivery whose attempt died: PROCESSING to PROCESSING with a
/// fresh claim time, only when the current claim was made before
/// `claimed_before`. The winner's new claim time takes every other taker out
/// of the condition, so `true` means "I won".
pub async fn reclaim_stale_delivery<'e, E>(
    ex: E,
    id: &str,
    created_at: DateTime<Utc>,
    claimed_before: DateTime<Utc>,
) -> Result<bool, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    let rows = run_transition(
        ex,
        &RECLAIM,
        &Selector::One { id, created_at },
        &[Change::StampLastAttempt],
        &Extra::ClaimedBefore(claimed_before),
    )
    .await?;
    Ok(!rows.is_empty())
}

/// An operator cancels a job: FAILED to CANCELLED, stamping `completed_at`.
/// The FAILED check is in the statement, so a job in any other status is
/// left alone.
pub async fn operator_cancel<'e, E>(
    ex: E,
    id: &str,
    created_at: DateTime<Utc>,
) -> Result<bool, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    settle_failed(ex, &OPERATOR_CANCEL, id, created_at).await
}

/// An operator completes a FAILED job by hand: FAILED to COMPLETED,
/// stamping `completed_at`. A job in any other status is left alone.
pub async fn operator_complete<'e, E>(
    ex: E,
    id: &str,
    created_at: DateTime<Utc>,
) -> Result<bool, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    settle_failed(ex, &OPERATOR_COMPLETE, id, created_at).await
}

async fn settle_failed<'e, E>(
    ex: E,
    t: &Transition,
    id: &str,
    created_at: DateTime<Utc>,
) -> Result<bool, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    let rows = run_transition(
        ex,
        t,
        &Selector::One { id, created_at },
        &[Change::StampCompletedAt],
        &Extra::None,
    )
    .await?;
    if rows.is_empty() {
        warn!(
            transition = t.name,
            id, "operator transition refused: the job is not FAILED"
        );
    }
    Ok(!rows.is_empty())
}

// ─── The claim and the hold-back (reads) ────────────────────────────────────

// How a job is claimed, and which jobs hold a message group, are READS of the
// job table: the scheduler keeps a job it is publishing out of the next claim
// with its own in-memory in-flight set, and a job that is not published is
// simply still PENDING.

/// One claimed PENDING job: what the scheduler needs to publish it, and the
/// `version` (the job's `updated_at`) its mark-QUEUED must still find.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct ClaimRow {
    pub job_id: String,
    pub job_created_at: DateTime<Utc>,
    pub message_group: Option<String>,
    pub sequence: i32,
    pub scheduled_for: Option<DateTime<Utc>>,
    pub subscription_id: Option<String>,
    pub dispatch_pool_id: Option<String>,
    pub client_id: Option<String>,
    pub mode: String,
    pub queue: Option<String>,
    pub version: DateTime<Utc>,
}

/// The holding statuses of a job that holds its message group: terminally
/// failed. (A PENDING job in a retry backoff holds it too; see
/// [`GROUP_HOLDERS_SQL`].)
pub const HOLDING_STATUSES: [&str; 2] = ["FAILED", "ERROR"];

/// The claim: up to `$1` due PENDING jobs in claim order, minus paused
/// subscriptions (`$2`), the groups the poller remembers as held (`$3`) and
/// the ids this process is already publishing (`$4`). One plain `SELECT`: no
/// lock, no transaction, no write. It walks `idx_dispatch_jobs_status_group`
/// (status equality prefix, then the index's own order, a Merge Append across
/// partitions) with no Sort. The status is a literal; with the scheduler
/// pool's `force_custom_plan` a bind plans the same.
const CLAIM_SQL: &str = "\
SELECT id AS job_id, created_at AS job_created_at, message_group, sequence, scheduled_for, \
       subscription_id, dispatch_pool_id, client_id, mode, queue, updated_at AS version \
  FROM msg_dispatch_jobs \
 WHERE status = 'PENDING' \
   AND (scheduled_for IS NULL OR scheduled_for <= NOW()) \
   AND (subscription_id IS NULL OR subscription_id <> ALL($2::text[])) \
   AND (message_group IS NULL OR message_group <> ALL($3::text[])) \
   AND id <> ALL($4::text[]) \
 ORDER BY message_group NULLS LAST, sequence, created_at, id \
 LIMIT $1";

/// The scheduler's claim (see [`CLAIM_SQL`]). Run it on the scheduler's pool
/// (`plan_cache_mode = force_custom_plan`, `enable_sort = off`). The arrays
/// are always bound (empty when there is nothing to exclude).
pub async fn claim(
    pool: &sqlx::PgPool,
    limit: i64,
    paused_subscriptions: &[String],
    held_groups: &[String],
    in_flight: &[String],
) -> Result<Vec<ClaimRow>, Error> {
    sqlx::query_as(CLAIM_SQL)
        .bind(limit)
        .bind(paused_subscriptions)
        .bind(held_groups)
        .bind(in_flight)
        .fetch_all(pool)
        .await
}

/// Which of the claimed BLOCK_ON_ERROR rows are held: those with an EARLIER
/// job in their group (by sequence, `created_at`, id) that is FAILED / ERROR
/// or is PENDING in a retry backoff. One batched statement per claim returns,
/// per candidate group, its EARLIEST holder ([`GROUP_HOLDERS_SQL`]); a
/// candidate is held when that holder is before it. An ungrouped row is held
/// only by a group literally named `default` (Go's quirk, kept). Returns the
/// held job ids.
pub async fn held_among(pool: &sqlx::PgPool, claims: &[ClaimRow]) -> Result<Vec<String>, Error> {
    let candidates: Vec<&ClaimRow> = claims
        .iter()
        .filter(|c| c.mode == "BLOCK_ON_ERROR")
        .collect();
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let group_of = |c: &ClaimRow| c.message_group.clone().unwrap_or_else(|| "default".into());
    // Each group's LAST candidate in position order: only a holder positioned
    // before a candidate matters, so it bounds the backoff probe.
    let mut last: HashMap<String, (i32, DateTime<Utc>, &str)> = HashMap::new();
    for c in &candidates {
        let pos = (c.sequence, c.job_created_at, c.job_id.as_str());
        last.entry(group_of(c))
            .and_modify(|l| {
                if *l < pos {
                    *l = pos;
                }
            })
            .or_insert(pos);
    }
    let mut groups: Vec<&String> = last.keys().collect();
    groups.sort();
    let sequences: Vec<i32> = groups.iter().map(|g| last[*g].0).collect();
    let created: Vec<DateTime<Utc>> = groups.iter().map(|g| last[*g].1).collect();
    let last_ids: Vec<&str> = groups.iter().map(|g| last[*g].2).collect();
    let statuses: Vec<&str> = HOLDING_STATUSES.to_vec();
    let holders: Vec<(String, i32, DateTime<Utc>, String)> = sqlx::query_as(GROUP_HOLDERS_SQL)
        .bind(statuses)
        .bind(&groups)
        .bind(sequences)
        .bind(created)
        .bind(last_ids)
        .fetch_all(pool)
        .await?;
    let earliest: HashMap<&str, (i32, DateTime<Utc>, &str)> = holders
        .iter()
        .map(|(g, seq, at, id)| (g.as_str(), (*seq, *at, id.as_str())))
        .collect();
    Ok(candidates
        .into_iter()
        .filter(|c| {
            earliest
                .get(group_of(c).as_str())
                .is_some_and(|h| *h < (c.sequence, c.job_created_at, c.job_id.as_str()))
        })
        .map(|c| c.job_id.clone())
        .collect())
}

/// Per candidate group (`$2`), its EARLIEST holder as `(message_group,
/// sequence, created_at, id)`: FAILED / ERROR jobs (`$1`) by status and group,
/// and the first PENDING job in a retry backoff (a future `scheduled_for`).
/// Both read `idx_dispatch_jobs_status_group`. `$3..$5` are each group's LAST
/// candidate `(sequence, created_at, id)`, aligned with `$2`: the backoff half
/// is one bounded index probe per group (`LATERAL ... LIMIT 1`), because only
/// a holder positioned before a candidate matters; unbounded, it walks every
/// due PENDING job of the group looking for one that is not due.
const GROUP_HOLDERS_SQL: &str = "\
SELECT DISTINCT ON (message_group) message_group, sequence, created_at, id FROM ( \
    SELECT message_group::text, sequence, created_at, id::text \
      FROM msg_dispatch_jobs \
     WHERE status = ANY($1::text[]) AND message_group = ANY($2::text[]) \
    UNION ALL \
    SELECT h.message_group::text, h.sequence, h.created_at, h.id::text \
      FROM unnest($2::text[], $3::int[], $4::timestamptz[], $5::text[]) \
           AS g(grp, seq, created, id) \
     CROSS JOIN LATERAL ( \
          SELECT message_group, sequence, created_at, id \
            FROM msg_dispatch_jobs \
           WHERE status = 'PENDING' AND message_group = g.grp AND scheduled_for > NOW() \
             AND (sequence, created_at, id) < (g.seq, g.created, g.id) \
           ORDER BY sequence, created_at, id \
           LIMIT 1) h \
) h2 \
ORDER BY message_group, sequence, created_at, id";

/// The delivery-time half of the hold-back (same meaning as the claim's): an
/// EARLIER job of `group` (by sequence, `created_at`, id) is FAILED / ERROR,
/// or is PENDING in a retry backoff. Positional, so a holder is never held
/// by its own presence.
pub async fn group_held_before<'e, E>(
    ex: E,
    group: &str,
    sequence: i32,
    created_at: DateTime<Utc>,
    id: &str,
) -> Result<bool, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    let statuses: Vec<String> = HOLDING_STATUSES.iter().map(|s| (*s).to_string()).collect();
    let (held,): (bool,) = sqlx::query_as(GROUP_HELD_BEFORE_SQL)
        .bind(group)
        .bind(sequence)
        .bind(created_at)
        .bind(id)
        .bind(&statuses)
        .fetch_one(ex)
        .await?;
    Ok(held)
}

const GROUP_HELD_BEFORE_SQL: &str = "\
SELECT EXISTS (SELECT 1 FROM msg_dispatch_jobs \
                WHERE status = ANY($5::text[]) AND message_group = $1 \
                  AND (sequence, created_at, id) < ($2, $3, $4)) \
    OR EXISTS (SELECT 1 FROM msg_dispatch_jobs \
                WHERE status = 'PENDING' AND message_group = $1 AND scheduled_for > NOW() \
                  AND (sequence, created_at, id) < ($2, $3, $4))";

/// The PENDING backlog, bounded: how many PENDING jobs there are (counted up
/// to [`BACKLOG_CAP`]; `saturated` when there are more) and the `created_at`
/// of the first one in claim order. A read the leader samples for a gauge;
/// the count never scans a huge backlog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingBacklog {
    pub depth: i64,
    pub saturated: bool,
    pub oldest_created_at: Option<DateTime<Utc>>,
}

/// The backlog count saturates here ("100,000+").
pub const BACKLOG_CAP: i64 = 100_000;

const BACKLOG_COUNT_SQL: &str = "\
SELECT count(*) FROM (SELECT 1 FROM msg_dispatch_jobs WHERE status = 'PENDING' LIMIT 100001) s";

const BACKLOG_OLDEST_SQL: &str = "\
SELECT created_at FROM msg_dispatch_jobs WHERE status = 'PENDING' \
 ORDER BY message_group NULLS LAST, sequence, created_at, id LIMIT 1";

pub async fn pending_backlog(pool: &sqlx::PgPool) -> Result<PendingBacklog, Error> {
    let counted: i64 = sqlx::query_scalar(BACKLOG_COUNT_SQL)
        .fetch_one(pool)
        .await?;
    let oldest: Option<DateTime<Utc>> = sqlx::query_scalar(BACKLOG_OLDEST_SQL)
        .fetch_optional(pool)
        .await?;
    Ok(PendingBacklog {
        depth: counted.min(BACKLOG_CAP),
        saturated: counted > BACKLOG_CAP,
        oldest_created_at: oldest,
    })
}

// ─── Plans (for the plan tests) ─────────────────────────────────────────────

/// `EXPLAIN (FORMAT JSON)` of one of the lifecycle's statements, run with
/// real parameters on `ex`, for the plan tests. `which` names the statement:
/// `stale_queued`, `stale_processing`, `reap`, `mark_queued`,
/// `claim_for_delivery` or `schedule_retry`. With `analyze` the statement
/// RUNS (EXPLAIN ANALYZE): pass a transaction and roll it back.
#[doc(hidden)]
pub async fn explain_statement<'e, E>(
    ex: E,
    which: &str,
    analyze: bool,
) -> Result<serde_json::Value, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    let at = Utc::now() - chrono::Duration::minutes(15);
    let key = [("x".to_string(), at, at)];
    let prefix = if analyze {
        "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) "
    } else {
        "EXPLAIN (FORMAT JSON) "
    };
    let (t, sel, changes): (&Transition, Selector<'_>, Vec<Change<'_>>) = match which {
        "stale_queued" => (
            &STALE_QUEUED,
            Selector::StaleSince { before: at },
            vec![Change::ClearQueuedAt],
        ),
        "stale_processing" => (
            &STALE_PROCESSING,
            Selector::StaleSince { before: at },
            vec![Change::ClearQueuedAt, Change::LastError(Some("plan"))],
        ),
        "reap" => (
            &REAP,
            Selector::StrandedSiblings { live_before: at },
            vec![
                Change::ScheduledFor(None),
                Change::ClearQueuedAt,
                Change::LastError(Some("plan")),
            ],
        ),
        "mark_queued" => (
            &MARK_QUEUED,
            Selector::Versioned(&key),
            vec![Change::StampQueuedAt],
        ),
        "claim_for_delivery" => (
            &CLAIM,
            Selector::One {
                id: "x",
                created_at: at,
            },
            vec![Change::StampLastAttempt],
        ),
        "schedule_retry" => (
            &RETRY,
            Selector::One {
                id: "x",
                created_at: at,
            },
            vec![Change::BumpAttemptCount, Change::ScheduledFor(Some(at))],
        ),
        other => panic!("unknown statement {other}"),
    };
    let mut qb = build_update_after(t, &sel, &changes, &Extra::None, prefix);
    let plan: serde_json::Value = qb.build_query_scalar().fetch_one(ex).await?;
    Ok(plan)
}

/// `EXPLAIN (FORMAT JSON)` of mark-QUEUED for exactly these claimed
/// `(id, created_at, version)` triples, for the plan tests (with `analyze`
/// the statement RUNS: pass a transaction and roll it back).
#[doc(hidden)]
pub async fn explain_mark_queued<'e, E>(
    ex: E,
    claimed: &[(String, DateTime<Utc>, DateTime<Utc>)],
    analyze: bool,
) -> Result<serde_json::Value, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    let prefix = if analyze {
        "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) "
    } else {
        "EXPLAIN (FORMAT JSON) "
    };
    let mut qb = build_update_after(
        &MARK_QUEUED,
        &Selector::Versioned(claimed),
        &[Change::StampQueuedAt],
        &Extra::None,
        prefix,
    );
    qb.build_query_scalar().fetch_one(ex).await
}

/// The SQL of the read statements, for the plan tests: `(name, sql)`,
/// parameters as `$n`.
#[doc(hidden)]
pub fn read_statements() -> Vec<(&'static str, &'static str)> {
    vec![
        ("claim", CLAIM_SQL),
        ("group_holders", GROUP_HOLDERS_SQL),
        ("group_held_before", GROUP_HELD_BEFORE_SQL),
        ("backlog_count", BACKLOG_COUNT_SQL),
        ("backlog_oldest", BACKLOG_OLDEST_SQL),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use DispatchStatus::{Cancelled, Completed, Expired, Failed, Pending, Processing, Queued};

    const ALL: [DispatchStatus; 7] = [
        Pending, Queued, Processing, Completed, Failed, Cancelled, Expired,
    ];

    /// The whole lifecycle in one place: for every transition, the statuses
    /// it moves and where they end; every other status is refused. This
    /// table is the documentation of the status column.
    #[test]
    fn the_lifecycle_table() {
        // (transition, [(from, to)]); statuses not listed are refused.
        let expected: &[(&str, &[(DispatchStatus, DispatchStatus)])] = &[
            (
                "retry",
                &[(Pending, Pending), (Queued, Pending), (Processing, Pending)],
            ),
            (
                "defer",
                &[(Pending, Pending), (Queued, Pending), (Processing, Pending)],
            ),
            (
                "hold",
                &[(Pending, Pending), (Queued, Pending), (Processing, Pending)],
            ),
            ("settle_return", &[(Queued, Pending), (Processing, Pending)]),
            ("reap", &[(Queued, Pending), (Processing, Pending)]),
            ("stale_queued", &[(Queued, Pending)]),
            ("stale_processing", &[(Processing, Pending)]),
            (
                "requeue",
                &[
                    (Pending, Pending),
                    (Queued, Pending),
                    (Processing, Pending),
                    (Completed, Pending),
                    (Failed, Pending),
                    (Cancelled, Pending),
                    (Expired, Pending),
                ],
            ),
            ("mark_queued", &[(Pending, Queued)]),
            ("claim", &[(Pending, Processing), (Queued, Processing)]),
            (
                "complete",
                &[
                    (Pending, Completed),
                    (Queued, Completed),
                    (Processing, Completed),
                ],
            ),
            (
                "fail",
                &[(Pending, Failed), (Queued, Failed), (Processing, Failed)],
            ),
            ("reclaim", &[(Processing, Processing)]),
            ("operator_cancel", &[(Failed, Cancelled)]),
            ("operator_complete", &[(Failed, Completed)]),
        ];
        assert_eq!(
            expected.len(),
            TRANSITIONS.len(),
            "every transition is listed"
        );
        for t in TRANSITIONS {
            let (_, rows) = expected
                .iter()
                .find(|(n, _)| *n == t.name)
                .unwrap_or_else(|| panic!("transition {} is not in the table", t.name));
            for from in ALL {
                let want = rows.iter().find(|(f, _)| *f == from).map(|(_, to)| *to);
                assert_eq!(t.apply(from), want, "{} from {}", t.name, from.as_str());
            }
        }
    }

    /// No transition other than the operator requeue may resurrect a
    /// settled job.
    #[test]
    fn only_requeue_touches_a_settled_job_into_pending() {
        for t in TRANSITIONS {
            if t.name == "requeue" {
                continue;
            }
            for settled in [Completed, Cancelled, Expired] {
                assert_eq!(
                    t.apply(settled),
                    None,
                    "{} from {}",
                    t.name,
                    settled.as_str()
                );
            }
            if t.name != "operator_cancel" && t.name != "operator_complete" {
                assert_eq!(t.apply(Failed), None, "{} from FAILED", t.name);
            }
        }
    }

    #[test]
    fn the_guard_is_literal_sql() {
        assert_eq!(
            STALE_QUEUED.guard_sql(false).as_deref(),
            Some("j.status = 'QUEUED'")
        );
        assert_eq!(
            CLAIM.guard_sql(false).as_deref(),
            Some("j.status IN ('PENDING', 'QUEUED')")
        );
        // A statement that pins its rows by primary key cannot let the
        // planner use the guard as an index condition.
        assert_eq!(
            STALE_QUEUED.guard_sql(true).as_deref(),
            Some("j.status || '' = 'QUEUED'")
        );
        assert_eq!(
            CLAIM.guard_sql(true).as_deref(),
            Some("j.status || '' IN ('PENDING', 'QUEUED')")
        );
        assert_eq!(REQUEUE.guard_sql(false), None);
    }

    /// Statement text for each transition: one UPDATE, a guard on the
    /// allowed statuses, `updated_at` stamped, the step-2 columns returned.
    #[test]
    fn every_statement_is_guarded_stamped_and_returns() {
        let at = Utc::now();
        let key = vec![("a".to_string(), at)];
        let ver = vec![("a".to_string(), at, at)];
        let ids = vec!["a".to_string()];
        let cases: Vec<(&Transition, Selector<'_>, Vec<Change<'_>>)> = vec![
            (
                &RETRY,
                Selector::One {
                    id: "a",
                    created_at: at,
                },
                vec![Change::BumpAttemptCount],
            ),
            (
                &DEFER,
                Selector::One {
                    id: "a",
                    created_at: at,
                },
                vec![Change::ClearQueuedAt],
            ),
            (
                &HOLD,
                Selector::One {
                    id: "a",
                    created_at: at,
                },
                vec![Change::ClearQueuedAt],
            ),
            (
                &SETTLE_RETURN,
                Selector::Ids(&ids),
                vec![Change::ClearQueuedAt],
            ),
            (
                &REAP,
                Selector::StrandedSiblings { live_before: at },
                vec![Change::ClearQueuedAt],
            ),
            (
                &STALE_QUEUED,
                Selector::StaleSince { before: at },
                vec![Change::ClearQueuedAt],
            ),
            (
                &STALE_PROCESSING,
                Selector::StaleSince { before: at },
                vec![Change::ClearQueuedAt],
            ),
            (
                &REQUEUE,
                Selector::Keys(&key),
                vec![Change::ResetAttemptCount],
            ),
            (
                &MARK_QUEUED,
                Selector::Versioned(&ver),
                vec![Change::StampQueuedAt],
            ),
            (
                &CLAIM,
                Selector::One {
                    id: "a",
                    created_at: at,
                },
                vec![Change::StampLastAttempt],
            ),
            (
                &COMPLETE,
                Selector::One {
                    id: "a",
                    created_at: at,
                },
                vec![Change::StampCompletedAt],
            ),
            (
                &FAIL,
                Selector::One {
                    id: "a",
                    created_at: at,
                },
                vec![Change::StampCompletedAt],
            ),
        ];
        for (t, sel, changes) in cases {
            let sql = render(t, &sel, &changes);
            assert_eq!(
                sql.matches("UPDATE msg_dispatch_jobs AS j").count(),
                1,
                "{sql}"
            );
            assert!(
                sql.contains(&format!("SET status = '{}'", t.to.as_str())),
                "{sql}"
            );
            assert!(sql.contains("updated_at = NOW()"), "{}: {sql}", t.name);
            assert!(
                sql.contains("RETURNING j.id, j.created_at"),
                "{}: {sql}",
                t.name
            );
            match t.guard_sql(sel.pins_by_key()) {
                Some(g) => assert!(sql.contains(&g), "{}: {sql}", t.name),
                None => assert_eq!(t.name, "requeue"),
            }
        }
    }

    /// A refused transition is counted, labelled by transition: one per
    /// addressed row that did not move; a sweep matching nothing is not a
    /// refusal.
    #[test]
    fn a_refused_transition_is_counted_by_label() {
        use metrics::{
            Counter, CounterFn, Gauge, Histogram, Key, KeyName, Metadata, Recorder, SharedString,
            Unit,
        };
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        #[derive(Default)]
        struct Counts(Mutex<HashMap<String, u64>>);
        struct Handle(String, Arc<Counts>);
        impl CounterFn for Handle {
            fn increment(&self, v: u64) {
                *self.1 .0.lock().unwrap().entry(self.0.clone()).or_default() += v;
            }
            fn absolute(&self, _v: u64) {}
        }
        struct Rec(Arc<Counts>);
        impl Recorder for Rec {
            fn describe_counter(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
            fn describe_gauge(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
            fn describe_histogram(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
            fn register_counter(&self, key: &Key, _: &Metadata<'_>) -> Counter {
                let label = key
                    .labels()
                    .map(|l| format!("{}={}", l.key(), l.value()))
                    .collect::<Vec<_>>()
                    .join(",");
                Counter::from_arc(Arc::new(Handle(
                    format!("{}|{label}", key.name()),
                    self.0.clone(),
                )))
            }
            fn register_gauge(&self, _: &Key, _: &Metadata<'_>) -> Gauge {
                Gauge::noop()
            }
            fn register_histogram(&self, _: &Key, _: &Metadata<'_>) -> Histogram {
                Histogram::noop()
            }
        }

        let counts = Arc::new(Counts::default());
        metrics::with_local_recorder(&Rec(counts.clone()), || {
            record_outcome(&COMPLETE, Some(1), 0); // refused
            record_outcome(&COMPLETE, Some(1), 1); // moved
            record_outcome(&MARK_QUEUED, Some(5), 3); // two refused
            record_outcome(&REAP, None, 0); // a sweep: not a refusal
        });
        let got = counts.0.lock().unwrap().clone();
        let key = |t: &str| format!("dispatch_job.transition_refused_total|transition={t}");
        assert_eq!(got.get(&key("complete")), Some(&1));
        assert_eq!(got.get(&key("mark_queued")), Some(&2));
        assert_eq!(got.get(&key("reap")), None);
    }

    fn flat(sql: &str) -> String {
        sql.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn selector_for<'a>(
        t: &Transition,
        key: &'a [(String, DateTime<Utc>)],
        ver: &'a [(String, DateTime<Utc>, DateTime<Utc>)],
        ids: &'a [String],
        at: DateTime<Utc>,
    ) -> Selector<'a> {
        match t.name {
            "settle_return" => Selector::Ids(ids),
            "reap" => Selector::StrandedSiblings { live_before: at },
            "stale_queued" | "stale_processing" => Selector::StaleSince { before: at },
            "requeue" => Selector::Keys(key),
            "mark_queued" => Selector::Versioned(ver),
            _ => Selector::One {
                id: "a",
                created_at: at,
            },
        }
    }

    /// mark-QUEUED, exact: each `(id, created_at)` is read by primary key in
    /// a fenced LATERAL sub-query (no status test inside it), version and
    /// status are checked on its result, and the UPDATE joins the matches
    /// back by the same key AND version with an OPAQUE status guard, so the status index
    /// is never an access path whatever the statistics say.
    #[test]
    fn mark_queued_statement_reads_by_primary_key_and_its_status_guard_is_opaque() {
        let at = Utc::now();
        let ver = vec![("a".to_string(), at, at)];
        let sql = flat(&render(
            &MARK_QUEUED,
            &Selector::Versioned(&ver),
            &[Change::StampQueuedAt],
        ));
        let expected = "WITH pk AS ( SELECT p.id, p.created_at, c.v FROM UNNEST($1::varchar[], \
            $2::timestamptz[], $3::timestamptz[]) AS c(id, created_at, v) \
            CROSS JOIN LATERAL ( SELECT id, created_at, updated_at, status \
            FROM msg_dispatch_jobs WHERE id = c.id AND created_at = c.created_at \
            OFFSET 0 ) p WHERE p.updated_at = c.v AND p.status = 'PENDING' ) \
            UPDATE msg_dispatch_jobs AS j SET status = 'QUEUED', queued_at = NOW(), \
            updated_at = NOW() FROM pk WHERE j.id = pk.id AND j.created_at = pk.created_at \
            AND j.updated_at = ( SELECT k.v FROM pk k WHERE k.id = j.id \
            AND k.created_at = j.created_at LIMIT 1 ) AND j.status || '' = 'PENDING' RETURNING j.id, j.created_at, j.message_group, \
            j.sequence, j.scheduled_for, j.subscription_id, j.dispatch_pool_id, j.client_id, \
            j.mode, j.queue, j.updated_at";
        assert_eq!(sql, flat(expected));
    }

    /// Every statement that pins its rows by primary key writes its status
    /// guard opaque; the sweeps, which SELECT BY status, keep it sargable.
    /// (This fails if a guard is made sargable again.)
    #[test]
    fn key_pinned_statements_have_opaque_status_guards_and_sweeps_keep_the_plain_one() {
        let at = Utc::now();
        let key = vec![("a".to_string(), at)];
        let ver = vec![("a".to_string(), at, at)];
        let ids = vec!["a".to_string()];
        for t in TRANSITIONS {
            let sel = selector_for(t, &key, &ver, &ids, at);
            let sql = flat(&render(t, &sel, &[]));
            let guarded = !t.from.is_empty();
            if sel.pins_by_key() && guarded {
                assert!(sql.contains("j.status || ''"), "{}: {sql}", t.name);
                assert!(!sql.contains(" AND j.status = '"), "{}: {sql}", t.name);
                assert!(!sql.contains(" AND j.status IN ("), "{}: {sql}", t.name);
            } else if guarded {
                assert!(!sql.contains("j.status || ''"), "{}: {sql}", t.name);
                assert!(
                    sql.contains("j.status = '") || sql.contains("j.status IN ("),
                    "{}: {sql}",
                    t.name
                );
            }
        }
    }

    /// Nothing the lifecycle issues mentions the retired queue table.
    #[test]
    fn no_statement_mentions_the_queue_table() {
        let at = Utc::now();
        let key = vec![("a".to_string(), at)];
        let ver = vec![("a".to_string(), at, at)];
        let ids = vec!["a".to_string()];
        for t in TRANSITIONS {
            let sql = render(t, &selector_for(t, &key, &ver, &ids, at), &[]);
            assert!(!sql.contains("msg_dispatch_queue"), "{}: {sql}", t.name);
            assert!(!sql.contains("WITH moved"), "{}: {sql}", t.name);
        }
        for sql in [create_batch_sql(), create_fanned_out_sql()] {
            assert!(!sql.contains("msg_dispatch_queue"), "{sql}");
        }
        for (name, sql) in read_statements() {
            assert!(!sql.contains("msg_dispatch_queue"), "{name}: {sql}");
        }
    }

    /// Creation is a plain INSERT of PENDING jobs.
    #[test]
    fn creation_statements_are_plain_inserts_of_pending_jobs() {
        for (name, sql) in [
            ("batch", flat(&create_batch_sql())),
            ("fan-out", flat(&create_fanned_out_sql())),
        ] {
            assert!(
                sql.starts_with("INSERT INTO msg_dispatch_jobs"),
                "{name}: {sql}"
            );
            assert!(sql.contains("'PENDING'"), "{name}: {sql}");
            assert!(!sql.contains("WITH ins"), "{name}: {sql}");
        }
        // The batch binds 37 arrays (no status) and the fan-out 24.
        assert!(flat(&create_batch_sql()).contains("$37::varchar[]"));
        assert!(!flat(&create_batch_sql()).contains("$38"));
        assert!(flat(&create_fanned_out_sql()).contains("$24::jsonb[]"));
    }

    /// The claim is one plain SELECT of the job table: PENDING, due, not
    /// paused, not a remembered held group, not in flight; the plain index's
    /// order; no lock, no write.
    #[test]
    fn the_claim_is_one_plain_select_of_the_job_table() {
        let sql = flat(CLAIM_SQL);
        assert!(sql.starts_with("SELECT id AS job_id, created_at AS job_created_at,"));
        assert!(sql.contains("FROM msg_dispatch_jobs WHERE status = 'PENDING'"));
        assert!(sql.contains("(scheduled_for IS NULL OR scheduled_for <= NOW())"));
        assert!(sql.contains("(subscription_id IS NULL OR subscription_id <> ALL($2::text[]))"));
        assert!(sql.contains("(message_group IS NULL OR message_group <> ALL($3::text[]))"));
        assert!(sql.contains("id <> ALL($4::text[])"));
        assert!(
            sql.ends_with("ORDER BY message_group NULLS LAST, sequence, created_at, id LIMIT $1")
        );
        for banned in [
            "FOR UPDATE",
            "SKIP LOCKED",
            "INSERT",
            "UPDATE",
            "DELETE",
            "WITH ",
        ] {
            assert!(!sql.contains(banned), "{banned}: {sql}");
        }
    }

    /// Both hold-back statements read the job table: FAILED / ERROR by status
    /// and group, PENDING backoffs by status, group and `scheduled_for`; the
    /// claim-time one is one bounded probe per group.
    #[test]
    fn the_hold_back_reads_the_job_table_only() {
        let batch = flat(GROUP_HOLDERS_SQL);
        let check = flat(GROUP_HELD_BEFORE_SQL);
        for sql in [&batch, &check] {
            assert!(sql.contains("status = ANY($"), "{sql}");
            assert!(sql.contains("status = 'PENDING'"), "{sql}");
            assert!(sql.contains("scheduled_for > NOW()"), "{sql}");
        }
        assert!(batch.contains("SELECT DISTINCT ON (message_group)"));
        assert!(batch.contains("(sequence, created_at, id) < (g.seq, g.created, g.id) ORDER BY sequence, created_at, id LIMIT 1) h"));
        assert!(check.contains("(sequence, created_at, id) < ($2, $3, $4)"));
        assert_eq!(HOLDING_STATUSES, ["FAILED", "ERROR"]);
    }

    /// The backlog sample never scans a huge backlog: it counts at most
    /// 100,001 rows.
    #[test]
    fn the_backlog_count_is_bounded() {
        assert!(flat(BACKLOG_COUNT_SQL).contains("LIMIT 100001) s"));
        assert_eq!(BACKLOG_CAP, 100_000);
        assert!(flat(BACKLOG_OLDEST_SQL).ends_with("LIMIT 1"));
    }

    /// The reaper's `stranded` CTE leads its statement and reads by status.
    #[test]
    fn the_reaper_statement_is_a_stranded_cte_then_the_update() {
        let sql = flat(&render(
            &REAP,
            &Selector::StrandedSiblings {
                live_before: Utc::now(),
            },
            &[],
        ));
        assert!(sql.starts_with("WITH stranded AS ("), "{sql}");
        assert_eq!(sql.matches("WITH ").count(), 1, "{sql}");
        assert!(
            sql.contains(") UPDATE msg_dispatch_jobs AS j SET status = 'PENDING'"),
            "{sql}"
        );
        assert!(
            sql.contains("WHERE j.id = st.id AND j.created_at = st.created_at AND (j.status <> 'PROCESSING' OR j.updated_at < $"),
            "{sql}"
        );
        assert!(
            sql.contains(") AND j.status IN ('QUEUED', 'PROCESSING')"),
            "{sql}"
        );
    }

    fn render(t: &Transition, sel: &Selector<'_>, changes: &[Change<'_>]) -> String {
        build_update(t, sel, changes, &Extra::None)
            .sql()
            .to_string()
    }
}
