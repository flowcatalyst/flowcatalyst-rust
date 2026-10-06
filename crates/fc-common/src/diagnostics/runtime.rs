//! Tokio runtime figures, from the runtime's **stable** metrics API only
//! (no `tokio_unstable` needed): worker count, alive tasks, global queue
//! depth, and per worker the busy time and park counters.
//!
//! How to read them when something is stuck:
//!
//! - `alive_tasks` climbing without bound: tasks are spawned faster than
//!   they finish (a leak, or a downstream that stopped answering).
//! - `global_queue_depth` staying high: runnable tasks are waiting for a
//!   worker — the workers are saturated or blocked.
//! - a worker whose park counter stops moving while it is `active`: that
//!   worker has not gone idle for the whole window, so it is either
//!   saturated or blocked inside one poll (a blocking call on an async
//!   thread). Tokio only folds busy time into its totals when a worker
//!   parks, so a blocked worker shows *no* new busy time — the park counter
//!   is the reliable signal. [`sample`] measures exactly this over a short
//!   window.

use serde::Serialize;
use std::time::Duration;
use tokio::runtime::RuntimeMetrics;
use tokio::runtime::{Handle, RuntimeFlavor};
use tokio::time;

/// One worker thread.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerSnapshot {
    pub index: usize,
    /// Busy time folded in so far (updated when the worker parks).
    pub busy_seconds: f64,
    pub park_count: u64,
    /// `true` while the worker is running tasks; `false` while parked.
    pub active: bool,
    /// Filled by [`sample`]: share of the window the worker reported busy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub busy_ratio: Option<f64>,
    /// Filled by [`sample`]: whether the worker parked at least once in the
    /// window. `false` together with `active` means saturated or blocked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parked_in_window: Option<bool>,
}

/// The runtime as a whole.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokioSnapshot {
    /// `multi_thread` or `current_thread`.
    pub flavor: &'static str,
    pub workers: usize,
    pub alive_tasks: usize,
    pub global_queue_depth: usize,
    pub busy_seconds_total: f64,
    pub park_count_total: u64,
    pub worker: Vec<WorkerSnapshot>,
    /// Filled by [`sample`]: the window the ratios cover.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_millis: Option<u64>,
    /// Filled by [`sample`]: workers that were active and never parked in
    /// the window (saturated or blocked).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workers_never_parked: Option<Vec<usize>>,
}

fn flavor_name(f: RuntimeFlavor) -> &'static str {
    match f {
        RuntimeFlavor::CurrentThread => "current_thread",
        RuntimeFlavor::MultiThread => "multi_thread",
        _ => "other",
    }
}

/// Read the runtime's counters now. Allocation aside, this is a handful of
/// relaxed atomic loads per worker.
pub fn snapshot(handle: &Handle) -> TokioSnapshot {
    let m = handle.metrics();
    let workers = m.num_workers();
    let mut worker = Vec::with_capacity(workers);
    let mut busy_total = 0.0;
    let mut park_total = 0;
    for index in 0..workers {
        let (busy_seconds, park_count, active) = worker_counters(&m, index);
        busy_total += busy_seconds;
        park_total += park_count;
        worker.push(WorkerSnapshot {
            index,
            busy_seconds,
            park_count,
            active,
            busy_ratio: None,
            parked_in_window: None,
        });
    }
    TokioSnapshot {
        flavor: flavor_name(handle.runtime_flavor()),
        workers,
        alive_tasks: m.num_alive_tasks(),
        global_queue_depth: m.global_queue_depth(),
        busy_seconds_total: busy_total,
        park_count_total: park_total,
        worker,
        sample_millis: None,
        workers_never_parked: None,
    }
}

#[cfg(target_has_atomic = "64")]
fn worker_counters(m: &RuntimeMetrics, i: usize) -> (f64, u64, bool) {
    (
        m.worker_total_busy_duration(i).as_secs_f64(),
        m.worker_park_count(i),
        // Odd: parked; even: running.
        m.worker_park_unpark_count(i).is_multiple_of(2),
    )
}

#[cfg(not(target_has_atomic = "64"))]
fn worker_counters(_: &tokio::runtime::RuntimeMetrics, _: usize) -> (f64, u64, bool) {
    (0.0, 0, true)
}

/// Two snapshots `window` apart, the second annotated with each worker's
/// busy ratio and whether it parked in between. Waits on a timer, so it
/// needs a runtime that is not the one blocked; the router and fc-server
/// call it from an HTTP handler, which is what an operator wants anyway:
/// if the handler cannot run, that is the answer.
pub async fn sample(handle: &Handle, window: Duration) -> TokioSnapshot {
    let before = snapshot(handle);
    time::sleep(window).await;
    let mut after = snapshot(handle);
    let secs = window.as_secs_f64().max(f64::EPSILON);
    let mut never_parked = Vec::new();
    for w in &mut after.worker {
        if let Some(b) = before.worker.iter().find(|b| b.index == w.index) {
            let ratio = ((w.busy_seconds - b.busy_seconds) / secs).clamp(0.0, 1.0);
            w.busy_ratio = Some((ratio * 1000.0).round() / 1000.0);
            let parked = w.park_count > b.park_count || !w.active || !b.active;
            w.parked_in_window = Some(parked);
            if !parked {
                never_parked.push(w.index);
            }
        }
    }
    after.sample_millis = Some(window.as_millis() as u64);
    after.workers_never_parked = Some(never_parked);
    after
}

/// Text exposition style: classic Prometheus text (0.0.4) or OpenMetrics
/// (the function host's `/metrics`, whose counter families are declared
/// without the `_total` suffix and which ends in `# EOF`, written by the
/// caller).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exposition {
    Prometheus,
    OpenMetrics,
}

pub(crate) struct Writer<'a> {
    pub out: &'a mut String,
    pub style: Exposition,
}

impl Writer<'_> {
    pub fn gauge(&mut self, name: &str, help: &str, value: f64) {
        self.header(name, "gauge", help);
        self.sample(name, "", value);
    }

    /// A counter family; `name` carries no `_total`.
    pub fn counter(&mut self, name: &str, help: &str, value: f64) {
        self.header_counter(name, help);
        self.sample(&format!("{name}_total"), "", value);
    }

    #[expect(
        clippy::let_underscore_must_use,
        reason = "writing to a String cannot fail"
    )]
    pub fn header(&mut self, name: &str, kind: &str, help: &str) {
        use std::fmt::Write;
        let _ = writeln!(self.out, "# HELP {name} {help}");
        let _ = writeln!(self.out, "# TYPE {name} {kind}");
    }

    pub fn header_counter(&mut self, name: &str, help: &str) {
        let family = match self.style {
            Exposition::Prometheus => format!("{name}_total"),
            Exposition::OpenMetrics => name.to_string(),
        };
        self.header(&family, "counter", help);
    }

    #[expect(
        clippy::let_underscore_must_use,
        reason = "writing to a String cannot fail"
    )]
    pub fn sample(&mut self, name: &str, labels: &str, value: f64) {
        use std::fmt::Write;
        if labels.is_empty() {
            let _ = writeln!(self.out, "{name} {value}");
        } else {
            let _ = writeln!(self.out, "{name}{{{labels}}} {value}");
        }
    }
}

/// Append the runtime's series. Label cardinality is bounded by the worker
/// count (the machine's cores, or `TOKIO_WORKER_THREADS`).
pub(crate) fn render(s: &TokioSnapshot, w: &mut Writer<'_>) {
    w.gauge(
        "tokio_runtime_workers",
        "Worker threads of the main tokio runtime.",
        s.workers as f64,
    );
    w.gauge(
        "tokio_runtime_alive_tasks",
        "Tasks spawned on the runtime that have not finished.",
        s.alive_tasks as f64,
    );
    w.gauge(
        "tokio_runtime_global_queue_depth",
        "Runnable tasks waiting in the runtime's global (injection) queue.",
        s.global_queue_depth as f64,
    );
    w.header_counter(
        "tokio_runtime_worker_busy_seconds",
        "Time each worker spent running tasks (folded in when the worker parks).",
    );
    for worker in &s.worker {
        w.sample(
            "tokio_runtime_worker_busy_seconds_total",
            &format!("worker=\"{}\"", worker.index),
            worker.busy_seconds,
        );
    }
    w.header_counter(
        "tokio_runtime_worker_park",
        "Times each worker parked for lack of work; a worker whose count stops moving is saturated or blocked.",
    );
    for worker in &s.worker {
        w.sample(
            "tokio_runtime_worker_park_total",
            &format!("worker=\"{}\"", worker.index),
            worker.park_count as f64,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::Barrier;
    use std::thread;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_snapshot_sees_the_runtime() {
        let s = snapshot(&Handle::current());
        assert_eq!(s.flavor, "multi_thread");
        assert_eq!(s.workers, 2);
        assert_eq!(s.worker.len(), 2);
    }

    /// A worker held in a blocking call never parks during the window: the
    /// signature of a blocked async thread.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sampling_flags_a_blocked_worker() {
        let handle = Handle::current();
        let started = Arc::new(Barrier::new(2));
        let entered = started.clone();
        let blocker = tokio::spawn(async move {
            entered.wait();
            // Deliberately blocking an async worker for the whole window.
            thread::sleep(Duration::from_millis(600));
        });
        started.wait();
        let s = sample(&handle, Duration::from_millis(300)).await;
        let never = s.workers_never_parked.clone().unwrap();
        assert!(!never.is_empty(), "{s:?}");
        blocker.await.unwrap();
    }

    #[test]
    fn renders_both_styles() {
        let s = TokioSnapshot {
            flavor: "multi_thread",
            workers: 1,
            alive_tasks: 3,
            global_queue_depth: 0,
            busy_seconds_total: 1.5,
            park_count_total: 7,
            worker: vec![WorkerSnapshot {
                index: 0,
                busy_seconds: 1.5,
                park_count: 7,
                active: true,
                busy_ratio: None,
                parked_in_window: None,
            }],
            sample_millis: None,
            workers_never_parked: None,
        };
        let mut out = String::new();
        render(
            &s,
            &mut Writer {
                out: &mut out,
                style: Exposition::Prometheus,
            },
        );
        assert!(out.contains("# TYPE tokio_runtime_worker_park_total counter"));
        assert!(out.contains("tokio_runtime_worker_park_total{worker=\"0\"} 7"));
        let mut om = String::new();
        render(
            &s,
            &mut Writer {
                out: &mut om,
                style: Exposition::OpenMetrics,
            },
        );
        assert!(om.contains("# TYPE tokio_runtime_worker_park counter"));
        assert!(om.contains("tokio_runtime_worker_park_total{worker=\"0\"} 7"));
    }
}
