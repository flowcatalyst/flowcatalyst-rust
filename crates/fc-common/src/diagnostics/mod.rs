//! Runtime diagnostics shared by every FlowCatalyst binary: tokio and
//! process metrics, the panic hook, task supervision and task dumps.
//!
//! - [`render_prometheus`] appends the runtime and process series to a
//!   `/metrics` body; [`report`] is the same data as JSON for the
//!   authenticated `/diagnostics/runtime` endpoints.
//! - [`init`] installs the panic hook and records the start time; each
//!   binary calls it right after logging is set up.
//! - [`supervise`] restarts (or escalates) a background loop that panicked.
//! - [`taskdump`] dumps every task's async backtrace where the build
//!   supports it.
//!
//! Operator guide: `docs/operations/diagnosing-stuck-processes.md`.

pub mod panic;
pub mod process;
pub mod runtime;
pub mod supervise;
pub mod taskdump;

pub use panic::{install_panic_hook, panic_count};
pub use runtime::Exposition;
pub use supervise::{catch_panic, spawn_supervised, supervise, OnPanic};
pub use taskdump::{task_dump, TaskDumpError, TASKDUMP_AVAILABLE};

use serde::Serialize;
use std::time::Duration;
use tokio::runtime::Handle;

/// Install the panic hook and note the process start. Call once, after the
/// `tracing` subscriber is installed (earlier works too: until then a
/// panic still reaches stderr).
pub fn init() {
    process::mark_start();
    install_panic_hook();
}

/// Everything `/diagnostics/runtime` reports.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeReport {
    pub version: &'static str,
    pub tokio: Option<runtime::TokioSnapshot>,
    pub process: process::ProcessSnapshot,
    pub panics: u64,
    /// Panics caught per supervised background task.
    pub task_restarts: std::collections::BTreeMap<&'static str, u64>,
    pub taskdump_available: bool,
}

/// A report on `handle`'s runtime (the current one when `None`), sampled
/// over `window` so each worker's busy ratio and whether it parked are
/// known; `Duration::ZERO` skips the sampling.
pub async fn report(handle: Option<&Handle>, window: Duration) -> RuntimeReport {
    let handle = handle.cloned().or_else(|| Handle::try_current().ok());
    let tokio = match handle {
        Some(h) if !window.is_zero() => Some(runtime::sample(&h, window).await),
        Some(h) => Some(runtime::snapshot(&h)),
        None => None,
    };
    RuntimeReport {
        version: crate::BUILD_VERSION,
        tokio,
        process: process::snapshot(),
        panics: panic_count(),
        task_restarts: supervise::restart_counts().into_iter().collect(),
        taskdump_available: TASKDUMP_AVAILABLE,
    }
}

/// Append the runtime (`tokio_runtime_*`), process (`process_*`) and
/// panic/restart (`fc_*`) series for `handle`'s runtime — the current one
/// when `None` — to `out`. Label cardinality is bounded: worker index and
/// supervised-task name only.
pub fn render_prometheus(out: &mut String, handle: Option<&Handle>, style: Exposition) {
    let mut w = runtime::Writer { out, style };
    let handle = handle.cloned().or_else(|| Handle::try_current().ok());
    if let Some(h) = handle {
        runtime::render(&runtime::snapshot(&h), &mut w);
    }
    let p = process::snapshot();
    if let Some(v) = p.cpu_seconds {
        w.counter(
            "process_cpu_seconds",
            "Total user and system CPU time spent in seconds.",
            v,
        );
    }
    if let Some(v) = p.resident_memory_bytes {
        w.gauge(
            "process_resident_memory_bytes",
            "Resident memory size in bytes.",
            v as f64,
        );
    }
    if let Some(v) = p.max_resident_memory_bytes {
        w.gauge(
            "process_max_resident_memory_bytes",
            "Peak resident memory size in bytes.",
            v as f64,
        );
    }
    if let Some(v) = p.virtual_memory_bytes {
        w.gauge(
            "process_virtual_memory_bytes",
            "Virtual memory size in bytes.",
            v as f64,
        );
    }
    if let Some(v) = p.open_fds {
        w.gauge(
            "process_open_fds",
            "Number of open file descriptors.",
            v as f64,
        );
    }
    if let Some(v) = p.max_fds {
        w.gauge(
            "process_max_fds",
            "Maximum number of open file descriptors.",
            v as f64,
        );
    }
    if let Some(v) = p.threads {
        w.gauge("process_threads", "Number of OS threads.", v as f64);
    }
    w.gauge(
        "process_start_time_seconds",
        "Start time of the process since unix epoch in seconds.",
        p.start_time_seconds,
    );
    w.counter(
        "fc_process_panics",
        "Panics in this process (every thread), each logged with its backtrace and span context.",
        panic_count() as f64,
    );
    let restarts = supervise::restart_counts();
    if !restarts.is_empty() {
        w.header_counter(
            "fc_task_restarts",
            "Panics caught in supervised background tasks, by task.",
        );
        for (task, n) in restarts {
            w.sample(
                "fc_task_restarts_total",
                &format!("task=\"{task}\""),
                n as f64,
            );
        }
    }
}

/// [`render_prometheus`] into a fresh string.
pub fn prometheus_text(handle: Option<&Handle>, style: Exposition) -> String {
    let mut out = String::new();
    render_prometheus(&mut out, handle, style);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_exposition_carries_runtime_and_process_series() {
        let text = prometheus_text(None, Exposition::Prometheus);
        assert!(text.contains("tokio_runtime_workers "), "{text}");
        assert!(text.contains("tokio_runtime_alive_tasks "));
        assert!(text.contains("process_start_time_seconds "));
        assert!(text.contains("# TYPE fc_process_panics_total counter"));
        #[cfg(unix)]
        assert!(text.contains("process_cpu_seconds_total "));
    }

    #[tokio::test]
    async fn a_report_without_sampling_is_immediate() {
        let r = report(None, Duration::ZERO).await;
        assert!(r.tokio.is_some());
        assert_eq!(r.taskdump_available, TASKDUMP_AVAILABLE);
    }
}
