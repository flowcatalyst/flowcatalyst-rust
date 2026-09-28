//! Runtime diagnostics for a router that looks stuck (`/diagnostics/*`):
//! the process and tokio runtime, a task dump, "what is message X / group G
//! doing now", and the flight recorder's recent events.
//!
//! Every route sits behind the router API's guard like the monitoring
//! routes: the platform bearer with `platform:messaging:router:view`, and
//! `…:router:operate` for the task dump (it pauses the runtime while it
//! walks every task). They are never served anonymously: on a router whose
//! API is open outside dev mode (the transitional `AUTH_MODE=NONE`) they
//! answer 403. Operator guide: `docs/operations/diagnosing-stuck-processes.md`.

use std::time::Duration;

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use super::AppState;
use crate::flight_recorder::{EventFilter, EventKind, RecordedEvent};
use crate::manager::InFlightMessageInfo;
use crate::pool::BufferedMessage;

/// Longest sampling window `/diagnostics/runtime` accepts.
const MAX_SAMPLE: Duration = Duration::from_secs(10);
/// Default and ceiling of the task dump's own timeout.
const DEFAULT_DUMP_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_DUMP_TIMEOUT: Duration = Duration::from_secs(30);
/// Most events one query returns.
const MAX_EVENTS: usize = 2000;

/// The answer on a router whose API is open outside dev mode.
fn refused() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({
            "error": "DIAGNOSTICS_REQUIRE_AUTH",
            "message": "diagnostics are only served behind the router API's authentication \
                        (platform bearer, or dev mode); this router runs with AUTH_MODE=NONE",
        })),
    )
        .into_response()
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeQuery {
    /// Sampling window in ms (default 1000, 0 = none, max 10000): long
    /// enough to see which runtime workers never parked.
    sample_ms: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RouterFigures {
    in_flight: usize,
    mediating: usize,
    live_groups: usize,
    parked_groups: usize,
    pools: usize,
    flight_recorder: FlightRecorderFigures,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FlightRecorderFigures {
    capacity: usize,
    held: usize,
    recorded_total: u64,
}

/// `GET /diagnostics/runtime`: the tokio runtime (workers, alive tasks,
/// global queue depth, per-worker busy ratio and whether each parked during
/// the sample), the process (CPU, RSS, fds, threads), panics and supervised
/// task restarts, and the router's own figures.
pub(crate) async fn runtime_handler(
    State(state): State<AppState>,
    Query(q): Query<RuntimeQuery>,
) -> Response {
    if !state.diagnostics_allowed {
        return refused();
    }
    let window = Duration::from_millis(q.sample_ms.unwrap_or(1000)).min(MAX_SAMPLE);
    let report = fc_common::diagnostics::report(None, window).await;
    let groups = state.queue_manager.blocked_groups();
    let recorder = state.queue_manager.flight_recorder();
    let router = RouterFigures {
        in_flight: state.queue_manager.in_flight_count(),
        mediating: state.queue_manager.mediating_snapshot().len(),
        live_groups: groups.len(),
        parked_groups: groups
            .iter()
            .filter(|g| !g.working && g.buffered > 0)
            .count(),
        pools: state.queue_manager.get_pool_stats().len(),
        flight_recorder: FlightRecorderFigures {
            capacity: recorder.capacity(),
            held: recorder.len(),
            recorded_total: recorder.recorded_total(),
        },
    };
    Json(serde_json::json!({ "runtime": report, "router": router })).into_response()
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskDumpQuery {
    timeout_ms: Option<u64>,
}

/// `GET /diagnostics/task-dump`: every task's async backtrace as text.
/// 501 on a build without task dumps; 504 when a worker is blocked and the
/// runtime cannot pause (itself the diagnosis). Needs `router:operate`.
pub(crate) async fn task_dump_handler(
    State(state): State<AppState>,
    Query(q): Query<TaskDumpQuery>,
) -> Response {
    if !state.diagnostics_allowed {
        return refused();
    }
    let timeout = q
        .timeout_ms
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_DUMP_TIMEOUT)
        .min(MAX_DUMP_TIMEOUT);
    task_dump_response(timeout).await
}

/// A task dump of the current runtime as an HTTP answer (shared with
/// fc-server's metrics-port diagnostics).
pub async fn task_dump_response(timeout: Duration) -> Response {
    use fc_common::diagnostics::TaskDumpError;
    let handle = tokio::runtime::Handle::current();
    match fc_common::diagnostics::task_dump(&handle, timeout).await {
        Ok(text) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            text,
        )
            .into_response(),
        Err(e @ TaskDumpError::NotAvailable) => (
            StatusCode::NOT_IMPLEMENTED,
            Json(
                serde_json::json!({ "error": "TASK_DUMP_NOT_AVAILABLE", "message": e.to_string() }),
            ),
        )
            .into_response(),
        Err(e @ TaskDumpError::TimedOut(_)) => (
            StatusCode::GATEWAY_TIMEOUT,
            Json(serde_json::json!({ "error": "TASK_DUMP_TIMED_OUT", "message": e.to_string() })),
        )
            .into_response(),
    }
}

/// What a message is doing now.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MessageDiagnosis {
    message_id: String,
    /// `MEDIATING` (a worker is delivering it), `BUFFERED` (waiting behind
    /// its group's head), `RETRY_BACKOFF`, `TRACKED_IDLE` (tracked, not in
    /// a worker or a buffer: typically between a settle and the tracker
    /// cleanup, or a phantom entry), or `NOT_IN_PIPELINE` (see `history`).
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    tracker: Option<InFlightMessageInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mediating: Option<MediatingNow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    buffered: Option<BufferedMessage>,
    /// The flight recorder's events for it, oldest first.
    history: Vec<RecordedEvent>,
    /// The recorder's own state, so an empty history is read right.
    recorder_enabled: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MediatingNow {
    pool_code: String,
    group: String,
    queue: String,
    target: String,
    attempts: u32,
    elapsed_ms: u64,
}

/// `GET /diagnostics/messages/{messageId}`: whether the router holds the
/// message and where (in a worker, in a group buffer at which position,
/// only in the tracker), plus what the flight recorder saw happen to it —
/// which also answers for a message that has already left.
pub(crate) async fn message_handler(
    State(state): State<AppState>,
    Path(message_id): Path<String>,
) -> Response {
    if !state.diagnostics_allowed {
        return refused();
    }
    let qm = &state.queue_manager;
    let tracker = qm.lookup_in_flight_by_app_id(&message_id);
    let now = std::time::Instant::now();
    let mediating = qm
        .mediating_snapshot()
        .into_iter()
        .find(|e| e.message_id == message_id)
        .map(|e| MediatingNow {
            pool_code: e.pool_code,
            group: e.group,
            queue: e.queue,
            target: e.target,
            attempts: e.attempts,
            elapsed_ms: now.saturating_duration_since(e.mediated_at).as_millis() as u64,
        });
    let buffered = if mediating.is_none() {
        qm.find_buffered(&message_id)
    } else {
        None
    };
    let status = match (&mediating, &buffered, &tracker) {
        (Some(_), _, _) => "MEDIATING",
        (None, Some(_), _) => "BUFFERED",
        (None, None, Some(t)) if t.retrying => "RETRY_BACKOFF",
        (None, None, Some(_)) => "TRACKED_IDLE",
        (None, None, None) => "NOT_IN_PIPELINE",
    };
    let recorder = qm.flight_recorder();
    Json(MessageDiagnosis {
        message_id: message_id.clone(),
        status,
        tracker,
        mediating,
        buffered,
        history: recorder.for_message(&message_id),
        recorder_enabled: recorder.is_enabled(),
    })
    .into_response()
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GroupQuery {
    pool_code: Option<String>,
    limit: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GroupDiagnosis {
    group: String,
    /// One row per pool holding the group (normally one).
    pools: Vec<GroupInPool>,
    /// The flight recorder's recent events for the group, oldest first.
    history: Vec<RecordedEvent>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GroupInPool {
    pool_code: String,
    /// A drainer owns the group.
    working: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    parked_at: Option<chrono::DateTime<chrono::Utc>>,
    suppressed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    suppressed_until: Option<chrono::DateTime<chrono::Utc>>,
    concurrency: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    rate_limit_per_minute: Option<u32>,
    /// The message a worker is delivering now, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    in_worker: Option<MediatingNow>,
    /// Buffered behind it, head first: `[messageId, attempts]`.
    buffered: Vec<(String, u32)>,
}

/// `GET /diagnostics/groups/{group}`: on each pool holding the group,
/// whether a drainer owns it, what is in a worker now, what is buffered
/// behind it, and the group's recent history (dispatch outcomes and the
/// group decisions they led to).
pub(crate) async fn group_handler(
    State(state): State<AppState>,
    Path(group): Path<String>,
    Query(q): Query<GroupQuery>,
) -> Response {
    if !state.diagnostics_allowed {
        return refused();
    }
    let qm = &state.queue_manager;
    let pool_filter = q.pool_code.as_deref();
    let now = std::time::Instant::now();
    let in_workers: Vec<_> = qm
        .mediating_snapshot()
        .into_iter()
        .filter(|e| e.group == group)
        .collect();
    let buffers = qm.group_buffers(&group);
    let pools: Vec<GroupInPool> = qm
        .blocked_groups()
        .into_iter()
        .filter(|g| g.group == group)
        .filter(|g| pool_filter.is_none_or(|f| g.pool_code.eq_ignore_ascii_case(f)))
        .map(|g| {
            let in_worker = in_workers
                .iter()
                .find(|e| e.pool_code == g.pool_code)
                .map(|e| MediatingNow {
                    pool_code: e.pool_code.clone(),
                    group: e.group.clone(),
                    queue: e.queue.clone(),
                    target: e.target.clone(),
                    attempts: e.attempts,
                    elapsed_ms: now.saturating_duration_since(e.mediated_at).as_millis() as u64,
                });
            let buffered = buffers
                .iter()
                .find(|(p, _)| *p == g.pool_code)
                .map(|(_, b)| b.clone())
                .unwrap_or_default();
            GroupInPool {
                pool_code: g.pool_code,
                working: g.working,
                parked_at: g.parked_at,
                suppressed: g.suppressed,
                suppressed_until: g.suppressed_until,
                concurrency: g.concurrency,
                rate_limit_per_minute: g.rate_limit_per_minute,
                in_worker,
                buffered,
            }
        })
        .collect();
    let history = qm.flight_recorder().query(
        &EventFilter {
            group: Some(group.clone()),
            pool: q.pool_code.clone(),
            ..EventFilter::default()
        },
        q.limit.unwrap_or(200).min(MAX_EVENTS),
    );
    Json(GroupDiagnosis {
        group,
        pools,
        history,
    })
    .into_response()
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EventsQuery {
    message_id: Option<String>,
    group: Option<String>,
    pool_code: Option<String>,
    /// An [`EventKind`] name, e.g. `DISPATCH_FINISHED`, `NACKED`.
    kind: Option<String>,
    limit: Option<usize>,
}

/// `GET /diagnostics/events`: the flight recorder's most recent events,
/// filtered by message, group, pool and kind; oldest first.
pub(crate) async fn events_handler(
    State(state): State<AppState>,
    Query(q): Query<EventsQuery>,
) -> Response {
    if !state.diagnostics_allowed {
        return refused();
    }
    let kind = match q.kind.as_deref().filter(|k| !k.is_empty()) {
        None => None,
        Some(k) => match serde_json::from_value::<EventKind>(serde_json::Value::String(
            k.to_ascii_uppercase(),
        )) {
            Ok(kind) => Some(kind),
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": "INVALID_KIND",
                        "message": format!("unknown event kind {k:?}"),
                    })),
                )
                    .into_response()
            }
        },
    };
    let events = state.queue_manager.flight_recorder().query(
        &EventFilter {
            message_id: q.message_id,
            group: q.group,
            pool: q.pool_code,
            kind,
        },
        q.limit.unwrap_or(200).min(MAX_EVENTS),
    );
    Json(events).into_response()
}
