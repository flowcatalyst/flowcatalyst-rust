//! Tokio task dumps: the async "stack" of every task on the runtime, the
//! async equivalent of a JVM thread dump.
//!
//! `Handle::dump` exists only in a build compiled with `--cfg
//! tokio_unstable` and tokio's `taskdump` feature, on Linux x86/x86_64/
//! aarch64. Nothing builds that way by default: the production image is on
//! stable tokio (`Dockerfile`, `FC_TASKDUMP=0`; pass `FC_TASKDUMP=1` for a
//! debugging image), and every other build (local, CI, fc-dev) answers
//! [`TaskDumpError::NotAvailable`]. See
//! `docs/operations/diagnosing-stuck-processes.md` for why it is a build
//! choice rather than a runtime one.

use std::error;
use std::fmt;
use std::fmt::Display;
use std::fmt::Formatter;
use std::time::Duration;
use tokio::runtime::Handle;

/// Whether this binary can take task dumps.
pub const TASKDUMP_AVAILABLE: bool = cfg!(all(
    tokio_unstable,
    feature = "taskdump",
    target_os = "linux",
    any(
        target_arch = "aarch64",
        target_arch = "x86",
        target_arch = "x86_64",
        target_arch = "s390x"
    )
));

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskDumpError {
    /// Built without task-dump support.
    NotAvailable,
    /// The runtime did not stop its workers in time: at least one worker is
    /// blocked (not yielding), which is itself the diagnosis.
    TimedOut(Duration),
}

impl Display for TaskDumpError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TaskDumpError::NotAvailable => f.write_str(
                "task dumps are not available in this build: it needs --cfg tokio_unstable and \
                 the taskdump feature on Linux (the production image has them; see \
                 docs/operations/diagnosing-stuck-processes.md)",
            ),
            TaskDumpError::TimedOut(t) => write!(
                f,
                "the task dump did not complete within {}ms: a runtime worker is blocked and \
                 not yielding (a blocking call or a busy loop on an async thread); compare \
                 /diagnostics/runtime's workersNeverParked",
                t.as_millis()
            ),
        }
    }
}

impl error::Error for TaskDumpError {}

/// Dump every task on `handle`'s runtime as text: one block per task with
/// its id and async backtrace. Bounded by `timeout` (tokio's dump never
/// finishes while a worker is blocked).
#[cfg(all(
    tokio_unstable,
    feature = "taskdump",
    target_os = "linux",
    any(
        target_arch = "aarch64",
        target_arch = "x86",
        target_arch = "x86_64",
        target_arch = "s390x"
    )
))]
pub async fn task_dump(handle: &Handle, timeout: Duration) -> Result<String, TaskDumpError> {
    use std::fmt::Write;
    let dump = tokio::time::timeout(timeout, handle.dump())
        .await
        .map_err(|_| TaskDumpError::TimedOut(timeout))?;
    let tasks = dump.tasks();
    let mut out = String::new();
    let _ = writeln!(out, "tokio task dump: {} tasks", tasks.iter().count());
    for (i, task) in tasks.iter().enumerate() {
        let _ = writeln!(out, "\n--- task {} (id {}) ---", i, task.id());
        let _ = writeln!(out, "{}", task.trace());
    }
    Ok(out)
}

/// Dump every task on `handle`'s runtime as text. This build has no
/// task-dump support, so it always answers [`TaskDumpError::NotAvailable`].
#[cfg(not(all(
    tokio_unstable,
    feature = "taskdump",
    target_os = "linux",
    any(
        target_arch = "aarch64",
        target_arch = "x86",
        target_arch = "x86_64",
        target_arch = "s390x"
    )
)))]
pub async fn task_dump(_handle: &Handle, _timeout: Duration) -> Result<String, TaskDumpError> {
    Err(TaskDumpError::NotAvailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn answers_by_build() {
        // A task parked in a known place, for the dump to show.
        let parked = tokio::spawn(async {
            time::sleep(Duration::from_secs(60)).await;
        });
        let r = task_dump(&Handle::current(), Duration::from_secs(5)).await;
        if TASKDUMP_AVAILABLE {
            let text = r.unwrap();
            println!("{}", text.lines().take(40).collect::<Vec<_>>().join("\n"));
            assert!(text.starts_with("tokio task dump"));
            assert!(text.contains("sleep"), "the parked task's frame: {text}");
        } else {
            assert_eq!(r, Err(TaskDumpError::NotAvailable));
        }
        parked.abort();
    }
}
