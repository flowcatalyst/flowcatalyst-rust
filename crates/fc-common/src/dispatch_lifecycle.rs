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
//! Both `RETURNING` the columns a future queue table needs
//! ([`Transitioned`]). Transitions with no PENDING on either side
//! ([`reclaim_stale_delivery`], the operator's cancel and complete of a
//! FAILED job) are also operations here but do not use the primitives.
//!
//! [`TRANSITIONS`] states the whole lifecycle in one place; the tests at the
//! bottom pin it.
//!
//! # The queue table
//!
//! `msg_dispatch_queue` (migration 064) holds exactly one row for every job
//! whose status is PENDING, mirroring the job's current values (`version` is
//! the job's `updated_at`), and none for any other job. Nothing reads it for
//! dispatching yet. It is written ONLY here, by explicit application writes
//! (no triggers), each fused with the job write into ONE statement through a
//! data-modifying CTE, so the pair is atomic with no new transaction and no
//! extra round trip (a caller's transaction is still honoured):
//!
//! * creation (`create_batch`, `create_fanned_out`): `WITH ins AS (INSERT
//!   INTO msg_dispatch_jobs ... RETURNING ...) INSERT INTO
//!   msg_dispatch_queue ... FROM ins ON CONFLICT (job_id) DO NOTHING`. Every
//!   created job is PENDING.
//! * [`enter_pending`]: `WITH moved AS (UPDATE ... RETURNING ...) INSERT INTO
//!   msg_dispatch_queue ... FROM moved ON CONFLICT (job_id) DO UPDATE ...,
//!   claimed_at = NULL`. Entering (or re-entering) PENDING refreshes
//!   `version` and `scheduled_for` and resets `claimed_at`.
//! * [`leave_pending`]: `WITH moved AS (UPDATE ... RETURNING ...), gone AS
//!   (DELETE FROM msg_dispatch_queue ...) SELECT ... FROM moved`. Delete if
//!   exists: complete / fail / claim may or may not find the job PENDING.
//!   [`mark_queued`] additionally deletes the queue row of every
//!   `(id, claimed version)` of its batch, even when the job row itself did
//!   not match (status moved on without the queue row being refreshed); a
//!   queue row with a different version (the job re-entered PENDING since
//!   the claim) is left alone.
//! * a partition drop removes that partition's queue rows (fc-stream's
//!   partition manager).
//!
//! Transitions with PENDING on neither side (reclaim, operator cancel /
//! complete) do not touch the queue.
//!
//! ## A known, accepted anomaly
//!
//! Under READ COMMITTED a statement's CTEs see the snapshot taken when the
//! statement started. If an *enter* and a *leave* for the SAME job race, and
//! the leave blocks on the job's row lock behind the enter, the leave's
//! `DELETE` cannot see the queue row the enter has just inserted, so a queue
//! row can outlive a job that is no longer PENDING. The opposite error (a
//! PENDING job with no queue row: a lost job) cannot happen this way: an
//! enter that waits behind a leave re-inserts with `ON CONFLICT`, which does
//! see committed rows. The stale extra row is harmless and self-heals: the
//! scheduler's mark-QUEUED removes it (the rule above), and a later reconcile
//! sweep will. The hot paths are deliberately NOT wrapped in transactions to
//! avoid it. [`queue_drift`] counts both kinds of error, for tests and
//! diagnostics only.

use chrono::{DateTime, Utc};
use sqlx::{Error, Executor, FromRow, Postgres, QueryBuilder};
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

    /// `status IN (...)` with literal statuses (never a bind: the partial
    /// indexes on `status` only match a constant predicate).
    fn guard_sql(&self) -> Option<String> {
        match self.from {
            [] => None,
            [one] => Some(format!("j.status = '{}'", one.as_str())),
            many => {
                let list: Vec<String> = many.iter().map(|s| format!("'{}'", s.as_str())).collect();
                Some(format!("j.status IN ({})", list.join(", ")))
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
                let ids: Vec<String> = keys.iter().map(|k| k.0.clone()).collect();
                let created: Vec<DateTime<Utc>> = keys.iter().map(|k| k.1).collect();
                let updated: Vec<DateTime<Utc>> = keys.iter().map(|k| k.2).collect();
                qb.push("WITH claimed AS ( SELECT * FROM UNNEST(")
                    .push_bind(ids)
                    .push("::varchar[], ")
                    .push_bind(created)
                    .push("::timestamptz[], ")
                    .push_bind(updated)
                    .push("::timestamptz[]) AS t(id, created_at, updated_at) )");
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
                qb.push(" FROM claimed t");
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
                qb.push(
                    " WHERE j.id = t.id AND j.created_at = t.created_at \
                     AND j.updated_at = t.updated_at",
                );
            }
            Selector::StaleSince { before } => {
                qb.push(" WHERE j.updated_at < ").push_bind(*before);
            }
            Selector::StrandedSiblings { .. } => {
                qb.push(" WHERE j.id = st.id AND j.created_at = st.created_at");
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

/// How a transition's statement treats the queue table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wrap {
    /// PENDING on neither side: the bare `UPDATE`, no queue write.
    Plain,
    /// The job becomes (or is refreshed as) PENDING: upsert its queue row.
    Enter,
    /// The job may stop being PENDING: delete its queue row if present.
    Leave,
}

impl Wrap {
    /// The wrap a transition needs, from the table alone.
    fn of(t: &Transition) -> Wrap {
        if t.to == DispatchStatus::Pending {
            Wrap::Enter
        } else if t.from.is_empty() || t.from.contains(&DispatchStatus::Pending) {
            Wrap::Leave
        } else {
            Wrap::Plain
        }
    }
}

/// The queue row's columns, in `INSERT` order.
macro_rules! queue_cols {
    () => {
        "job_id, job_created_at, message_group, sequence, scheduled_for, subscription_id, \
         dispatch_pool_id, client_id, mode, queue, version"
    };
}

/// A CTE named `moved` / `ins` re-selected under the alias `m` / `i`.
macro_rules! select_moved {
    () => {
        "SELECT m.id, m.created_at, m.message_group, m.sequence, m.scheduled_for, \
         m.subscription_id, m.dispatch_pool_id, m.client_id, m.mode, m.queue, m.updated_at \
         FROM moved m"
    };
}

/// Enter: upsert the queue row from the moved rows. Entering (or
/// re-entering) PENDING refreshes everything the job carries and resets
/// `claimed_at`. Returns the queue rows in [`Transitioned`] shape.
const ENTER_TAIL: &str = concat!(
    " ) INSERT INTO msg_dispatch_queue (",
    queue_cols!(),
    ") ",
    select_moved!(),
    " ON CONFLICT (job_id) DO UPDATE SET \
     job_created_at = EXCLUDED.job_created_at, message_group = EXCLUDED.message_group, \
     sequence = EXCLUDED.sequence, scheduled_for = EXCLUDED.scheduled_for, \
     subscription_id = EXCLUDED.subscription_id, dispatch_pool_id = EXCLUDED.dispatch_pool_id, \
     client_id = EXCLUDED.client_id, mode = EXCLUDED.mode, queue = EXCLUDED.queue, \
     version = EXCLUDED.version, claimed_at = NULL \
     RETURNING job_id AS id, job_created_at AS created_at, message_group, sequence, \
     scheduled_for, subscription_id, dispatch_pool_id, client_id, mode, queue, \
     version AS updated_at"
);

/// Leave: delete the queue row of every moved job (if it has one), then
/// return the moved rows.
const LEAVE_TAIL: &str = concat!(
    " ), gone AS ( DELETE FROM msg_dispatch_queue q USING moved m WHERE q.job_id = m.id ) ",
    select_moved!()
);

/// Leave for the scheduler's mark-QUEUED: additionally deletes the queue row
/// of every `(id, claimed version)` of the batch even when the job row did
/// not match (status moved on without the queue row being refreshed). A row
/// whose version differs (the job re-entered PENDING since the claim) is left
/// alone.
const LEAVE_CLAIMED_TAIL: &str = concat!(
    " ), gone AS ( DELETE FROM msg_dispatch_queue q USING ( \
     SELECT m.id FROM moved m \
     UNION \
     SELECT c.id FROM claimed c \
       JOIN msg_dispatch_queue s ON s.job_id = c.id AND s.version = c.updated_at \
     ) d WHERE q.job_id = d.id ) ",
    select_moved!()
);

/// Builds ONE statement for `t`: the `UPDATE msg_dispatch_jobs` (status to
/// `t.to`, the guard on `t.from`, `updated_at = NOW()`, the selector) with its
/// `RETURNING` of [`Transitioned`], wrapped per `wrap` so the queue row is
/// written by the same statement.
fn build_update<'q>(
    t: &Transition,
    sel: &Selector<'_>,
    changes: &[Change<'_>],
    extra: &Extra,
    wrap: Wrap,
) -> QueryBuilder<'q, Postgres> {
    let mut qb: QueryBuilder<'q, Postgres> = QueryBuilder::new("");
    let with = sel.push_ctes(&mut qb);
    match (wrap, with) {
        (Wrap::Plain, true) => {
            qb.push(" ");
        }
        (Wrap::Plain, false) => {}
        (_, true) => {
            qb.push(", moved AS ( ");
        }
        (_, false) => {
            qb.push("WITH moved AS ( ");
        }
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
    if let Some(g) = t.guard_sql() {
        qb.push(" AND ").push(g);
    }
    if let Extra::ClaimedBefore(at) = extra {
        qb.push(" AND COALESCE(j.last_attempt_at, j.updated_at) < ")
            .push_bind(*at);
    }
    qb.push(RETURNING);
    match (wrap, sel) {
        (Wrap::Plain, _) => {}
        (Wrap::Enter, _) => {
            qb.push(ENTER_TAIL);
        }
        (Wrap::Leave, Selector::Versioned(_)) => {
            qb.push(LEAVE_CLAIMED_TAIL);
        }
        (Wrap::Leave, _) => {
            qb.push(LEAVE_TAIL);
        }
    }
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
    wrap: Wrap,
) -> Result<Vec<Transitioned>, Error>
where
    E: Executor<'e, Database = Postgres>,
{
    debug_assert_eq!(wrap, Wrap::of(t));
    // Nothing to address: no statement, nothing refused.
    if matches!(sel.expected(), Some(0)) {
        return Ok(Vec::new());
    }
    let mut qb = build_update(t, sel, changes, extra, wrap);
    let rows: Vec<Transitioned> = qb.build_query_as().fetch_all(ex).await?;
    record_outcome(t, sel.expected(), rows.len());
    Ok(rows)
}

/// The single place a job becomes (or is refreshed as) PENDING: the job's
/// `UPDATE` and its queue row's upsert are one statement.
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
    run_transition(ex, t, &sel, changes, &Extra::None, Wrap::Enter).await
}

/// The single place a job stops being PENDING: the job's `UPDATE` and the
/// delete of its queue row (if any) are one statement.
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
    run_transition(ex, t, &sel, changes, &Extra::None, Wrap::Leave).await
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

/// The queue row of every job a creation statement inserted (`ins` is the
/// `RETURNING` of the job insert). Only rows actually inserted get one.
const CREATE_TAIL: &str = concat!(
    " ) INSERT INTO msg_dispatch_queue (",
    queue_cols!(),
    ") SELECT i.id, i.created_at, i.message_group, i.sequence, i.scheduled_for, \
     i.subscription_id, i.dispatch_pool_id, i.client_id, i.mode, i.queue, i.updated_at \
     FROM ins i ON CONFLICT (job_id) DO NOTHING"
);

/// The job columns a creation statement returns for its queue row.
const CREATE_RETURNING: &str = " RETURNING id, created_at, message_group, sequence, \
     scheduled_for, subscription_id, dispatch_pool_id, client_id, mode, queue, updated_at";

/// The 37-column insert of [`create_batch`]: every job PENDING, and its
/// queue row, in one statement.
fn create_batch_sql() -> String {
    format!(
        "{}{}{}",
        r#"WITH ins AS ( INSERT INTO msg_dispatch_jobs
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
        CREATE_RETURNING,
        CREATE_TAIL,
    )
}

/// Insert jobs in full, always PENDING, one statement: the `UNNEST` insert
/// and the queue rows of the jobs it inserted. A single job is a batch of
/// one. Run it inside the caller's transaction when other writes must be
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

/// The fan-out insert of [`create_fanned_out`] and its queue rows, in one
/// statement.
fn create_fanned_out_sql() -> String {
    format!(
        "{}{}{}",
        r#"
        WITH ins AS ( INSERT INTO msg_dispatch_jobs (
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
        CREATE_RETURNING,
        CREATE_TAIL,
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
        Wrap::Plain,
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
        Wrap::Plain,
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

// ─── Drift check ────────────────────────────────────────────────────────────

/// What [`queue_drift`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueDrift {
    /// Jobs that are PENDING with no queue row, or whose queue row's
    /// `version` / `scheduled_for` differs from the job's. A lost job.
    pub missing_or_stale: i64,
    /// Queue rows whose job is not PENDING or does not exist. The documented
    /// enter / leave race can leave a few; they are harmless.
    pub orphaned: i64,
}

/// Counts how far `msg_dispatch_queue` has drifted from the jobs (two
/// queries). For tests and diagnostics: it scans, and is NOT called on any
/// hot path.
pub async fn queue_drift(pool: &sqlx::PgPool) -> Result<QueueDrift, Error> {
    let (missing_or_stale,): (i64,) = sqlx::query_as(MISSING_OR_STALE_SQL).fetch_one(pool).await?;
    let (orphaned,): (i64,) = sqlx::query_as(ORPHANED_SQL).fetch_one(pool).await?;
    Ok(QueueDrift {
        missing_or_stale,
        orphaned,
    })
}

const MISSING_OR_STALE_SQL: &str = "SELECT COUNT(*) FROM msg_dispatch_jobs j \
     LEFT JOIN msg_dispatch_queue q ON q.job_id = j.id \
     WHERE j.status = 'PENDING' \
       AND (q.job_id IS NULL \
            OR q.version IS DISTINCT FROM j.updated_at \
            OR q.scheduled_for IS DISTINCT FROM j.scheduled_for)";

const ORPHANED_SQL: &str = "SELECT COUNT(*) FROM msg_dispatch_queue q \
     WHERE NOT EXISTS (SELECT 1 FROM msg_dispatch_jobs j \
                        WHERE j.id = q.job_id AND j.created_at = q.job_created_at \
                          AND j.status = 'PENDING')";

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
            STALE_QUEUED.guard_sql().as_deref(),
            Some("j.status = 'QUEUED'")
        );
        assert_eq!(
            CLAIM.guard_sql().as_deref(),
            Some("j.status IN ('PENDING', 'QUEUED')")
        );
        assert_eq!(REQUEUE.guard_sql(), None);
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
            match t.guard_sql() {
                Some(g) => assert!(sql.contains(&g), "{}: {sql}", t.name),
                None => assert_eq!(t.name, "requeue"),
            }
        }
    }

    /// The scheduler's hot statement keeps its shape: PENDING only, the
    /// version it read, three UNNEST arrays, `queued_at` stamped.
    #[test]
    fn mark_queued_statement_is_the_versioned_pending_update() {
        let at = Utc::now();
        let ver = vec![("a".to_string(), at, at)];
        let sql = render(
            &MARK_QUEUED,
            &Selector::Versioned(&ver),
            &[Change::StampQueuedAt],
        );
        assert!(sql.contains("j.status = 'PENDING'"), "{sql}");
        assert!(sql.contains("AND j.updated_at = t.updated_at"), "{sql}");
        assert!(sql.contains("$3::timestamptz[]"), "{sql}");
        assert!(sql.contains("queued_at = NOW()"), "{sql}");
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

    /// Collapses whitespace so assertions do not depend on formatting.
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

    /// The queue is written by exactly the transitions with PENDING on a
    /// side: entering upserts, leaving deletes, the rest never mention it.
    #[test]
    fn the_queue_is_written_by_exactly_the_transitions_that_touch_pending() {
        let at = Utc::now();
        let key = vec![("a".to_string(), at)];
        let ver = vec![("a".to_string(), at, at)];
        let ids = vec!["a".to_string()];
        for t in TRANSITIONS {
            let sql = flat(&render(t, &selector_for(t, &key, &ver, &ids, at), &[]));
            let enters = t.to == Pending;
            let leaves = !enters && (t.from.is_empty() || t.from.contains(&Pending));
            assert_eq!(Wrap::of(t) == Wrap::Enter, enters, "{}", t.name);
            assert_eq!(Wrap::of(t) == Wrap::Leave, leaves, "{}", t.name);
            assert_eq!(
                sql.contains("INSERT INTO msg_dispatch_queue"),
                enters,
                "{}: {sql}",
                t.name
            );
            assert_eq!(
                sql.contains("DELETE FROM msg_dispatch_queue"),
                leaves,
                "{}: {sql}",
                t.name
            );
            if !enters && !leaves {
                assert!(!sql.contains("msg_dispatch_queue"), "{}: {sql}", t.name);
                assert!(sql.starts_with("UPDATE msg_dispatch_jobs"), "{sql}");
            }
        }
    }

    /// enterPending, exact: the job UPDATE is the `moved` CTE; the queue row
    /// is upserted from it; re-entering refreshes the version and
    /// scheduled_for and resets claimed_at; the caller gets the queue row.
    #[test]
    fn enter_pending_statement_is_the_update_cte_and_the_queue_upsert() {
        let at = Utc::now();
        let sql = flat(&render(
            &DEFER,
            &Selector::One {
                id: "a",
                created_at: at,
            },
            &[Change::ScheduledFor(Some(at))],
        ));
        let expected = "WITH moved AS ( UPDATE msg_dispatch_jobs AS j SET status = 'PENDING', \
            scheduled_for = $1, updated_at = NOW() WHERE j.id = $2 AND j.created_at = $3 \
            AND j.status IN ('PENDING', 'QUEUED', 'PROCESSING') RETURNING j.id, j.created_at, \
            j.message_group, j.sequence, j.scheduled_for, j.subscription_id, \
            j.dispatch_pool_id, j.client_id, j.mode, j.queue, j.updated_at ) \
            INSERT INTO msg_dispatch_queue (job_id, job_created_at, message_group, sequence, \
            scheduled_for, subscription_id, dispatch_pool_id, client_id, mode, queue, version) \
            SELECT m.id, m.created_at, m.message_group, m.sequence, m.scheduled_for, \
            m.subscription_id, m.dispatch_pool_id, m.client_id, m.mode, m.queue, m.updated_at \
            FROM moved m ON CONFLICT (job_id) DO UPDATE SET \
            job_created_at = EXCLUDED.job_created_at, message_group = EXCLUDED.message_group, \
            sequence = EXCLUDED.sequence, scheduled_for = EXCLUDED.scheduled_for, \
            subscription_id = EXCLUDED.subscription_id, \
            dispatch_pool_id = EXCLUDED.dispatch_pool_id, client_id = EXCLUDED.client_id, \
            mode = EXCLUDED.mode, queue = EXCLUDED.queue, version = EXCLUDED.version, \
            claimed_at = NULL RETURNING job_id AS id, job_created_at AS created_at, \
            message_group, sequence, scheduled_for, subscription_id, dispatch_pool_id, \
            client_id, mode, queue, version AS updated_at";
        assert_eq!(sql, flat(expected));
    }

    /// The reaper's `stranded` CTE and the `moved` CTE share one `WITH`.
    #[test]
    fn the_reaper_statement_chains_its_ctes() {
        let at = Utc::now();
        let sql = flat(&render(
            &REAP,
            &Selector::StrandedSiblings { live_before: at },
            &[],
        ));
        assert!(sql.starts_with("WITH stranded AS ("), "{sql}");
        assert_eq!(sql.matches("WITH ").count(), 1, "{sql}");
        assert!(sql.contains(") ), moved AS ( UPDATE"), "{sql}");
        assert!(sql.contains("INSERT INTO msg_dispatch_queue"), "{sql}");
    }

    /// leavePending, exact (a complete): the queue row is deleted by the
    /// same statement, which returns the moved rows.
    #[test]
    fn leave_pending_statement_is_the_update_cte_and_the_queue_delete() {
        let at = Utc::now();
        let sql = flat(&render(
            &COMPLETE,
            &Selector::One {
                id: "a",
                created_at: at,
            },
            &[Change::StampCompletedAt],
        ));
        let expected = "WITH moved AS ( UPDATE msg_dispatch_jobs AS j SET status = 'COMPLETED', \
            completed_at = NOW(), updated_at = NOW() WHERE j.id = $1 AND j.created_at = $2 \
            AND j.status IN ('PENDING', 'QUEUED', 'PROCESSING') RETURNING j.id, j.created_at, \
            j.message_group, j.sequence, j.scheduled_for, j.subscription_id, \
            j.dispatch_pool_id, j.client_id, j.mode, j.queue, j.updated_at ), \
            gone AS ( DELETE FROM msg_dispatch_queue q USING moved m WHERE q.job_id = m.id ) \
            SELECT m.id, m.created_at, m.message_group, m.sequence, m.scheduled_for, \
            m.subscription_id, m.dispatch_pool_id, m.client_id, m.mode, m.queue, m.updated_at \
            FROM moved m";
        assert_eq!(sql, flat(expected));
    }

    /// The scheduler's mark-QUEUED, exact: the claimed batch is a CTE; the
    /// UPDATE matches the version it read; the queue delete is keyed on the
    /// moved rows AND on the batch's (id, claimed version) pairs.
    #[test]
    fn mark_queued_statement_deletes_by_claimed_version_too() {
        let at = Utc::now();
        let ver = vec![("a".to_string(), at, at)];
        let sql = flat(&render(
            &MARK_QUEUED,
            &Selector::Versioned(&ver),
            &[Change::StampQueuedAt],
        ));
        let expected = "WITH claimed AS ( SELECT * FROM UNNEST($1::varchar[], \
            $2::timestamptz[], $3::timestamptz[]) AS t(id, created_at, updated_at) ), \
            moved AS ( UPDATE msg_dispatch_jobs AS j SET status = 'QUEUED', queued_at = NOW(), \
            updated_at = NOW() FROM claimed t WHERE j.id = t.id AND j.created_at = t.created_at \
            AND j.updated_at = t.updated_at AND j.status = 'PENDING' RETURNING j.id, \
            j.created_at, j.message_group, j.sequence, j.scheduled_for, j.subscription_id, \
            j.dispatch_pool_id, j.client_id, j.mode, j.queue, j.updated_at ), \
            gone AS ( DELETE FROM msg_dispatch_queue q USING ( SELECT m.id FROM moved m UNION \
            SELECT c.id FROM claimed c JOIN msg_dispatch_queue s ON s.job_id = c.id \
            AND s.version = c.updated_at ) d WHERE q.job_id = d.id ) \
            SELECT m.id, m.created_at, m.message_group, m.sequence, m.scheduled_for, \
            m.subscription_id, m.dispatch_pool_id, m.client_id, m.mode, m.queue, m.updated_at \
            FROM moved m";
        assert_eq!(sql, flat(expected));
    }

    /// Creation, both statements: every job PENDING, the queue row written
    /// from the rows actually inserted, a skipped row adds nothing.
    #[test]
    fn creation_statements_insert_pending_jobs_and_their_queue_rows() {
        for (name, sql) in [
            ("batch", flat(&create_batch_sql())),
            ("fan-out", flat(&create_fanned_out_sql())),
        ] {
            assert!(
                sql.starts_with("WITH ins AS ( INSERT INTO msg_dispatch_jobs"),
                "{name}: {sql}"
            );
            assert!(sql.contains("'PENDING'"), "{name}: {sql}");
            assert!(
                sql.contains(
                    "RETURNING id, created_at, message_group, sequence, scheduled_for, \
                     subscription_id, dispatch_pool_id, client_id, mode, queue, updated_at ) \
                     INSERT INTO msg_dispatch_queue (job_id, job_created_at, message_group, \
                     sequence, scheduled_for, subscription_id, dispatch_pool_id, client_id, \
                     mode, queue, version) SELECT i.id, i.created_at, i.message_group, \
                     i.sequence, i.scheduled_for, i.subscription_id, i.dispatch_pool_id, \
                     i.client_id, i.mode, i.queue, i.updated_at FROM ins i \
                     ON CONFLICT (job_id) DO NOTHING"
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .as_str()
                ),
                "{name}: {sql}"
            );
        }
        // The batch binds 37 arrays (no status) and the fan-out 24.
        assert!(flat(&create_batch_sql()).contains("$37::varchar[]"));
        assert!(!flat(&create_batch_sql()).contains("$38"));
        assert!(flat(&create_fanned_out_sql()).contains("$24::jsonb[]"));
    }

    /// The drift queries read; they never write.
    #[test]
    fn the_drift_queries_only_read() {
        for sql in [MISSING_OR_STALE_SQL, ORPHANED_SQL] {
            let sql = flat(sql);
            assert!(sql.starts_with("SELECT COUNT(*)"), "{sql}");
            for verb in ["INSERT", "UPDATE", "DELETE"] {
                assert!(!sql.contains(verb), "{sql}");
            }
        }
    }

    fn render(t: &Transition, sel: &Selector<'_>, changes: &[Change<'_>]) -> String {
        build_update(t, sel, changes, &Extra::None, Wrap::of(t))
            .sql()
            .to_string()
    }
}
