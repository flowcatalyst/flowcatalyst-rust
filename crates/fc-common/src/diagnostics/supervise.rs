//! Supervision for long-lived background loops (pollers, reapers,
//! watchdogs, sweepers).
//!
//! Tokio catches a panic at the task boundary and the task is simply gone:
//! nobody awaits a background loop's handle until shutdown, so a reaper or
//! a watchdog that panicked once stopped working for the rest of the
//! process's life without a trace. [`spawn_supervised`] runs the loop so a
//! panic is logged (the panic hook has already logged the payload, the
//! backtrace and the span context), counted in
//! `fc_task_restarts_total{task}`, and then handled by the component's
//! contract: restarted after a backoff, or the process exits so the
//! orchestrator replaces it.
//!
//! A loop that *returns* is finished (cancelled, or its source closed) and
//! is not restarted.

use std::collections::BTreeMap;
use std::future::Future;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use std::any::Any;
use std::process;
use tokio::task::JoinHandle;
use tokio::time;

/// What a panic in a supervised loop means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnPanic {
    /// Start the loop again after a backoff (1 s doubling to 60 s; back to
    /// 1 s once a run has lasted a minute).
    Restart,
    /// Exit the process with this code: the component cannot run without
    /// the loop and has no safe way to rebuild it in place.
    Exit(i32),
}

const FIRST_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const HEALTHY_RUN: Duration = Duration::from_secs(60);

static RESTARTS: Mutex<BTreeMap<&'static str, u64>> = Mutex::new(BTreeMap::new());

/// Panics caught per supervised task name (bounded: names are static).
pub fn restart_counts() -> Vec<(&'static str, u64)> {
    RESTARTS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .map(|(k, v)| (*k, *v))
        .collect()
}

/// Count a panic of the task `name` that was caught somewhere other than
/// [`spawn_supervised`] (a `JoinError` a caller inspected).
pub fn note_task_panic(name: &'static str) {
    *RESTARTS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .entry(name)
        .or_insert(0) += 1;
}

/// A future that turns a panic while polling into `Err(payload)`, so the
/// supervisor can act on it inside the same task (an abort of the
/// supervisor's handle therefore still stops the loop).
struct CatchUnwind<F>(F);

impl<F: Future> Future for CatchUnwind<F> {
    type Output = Result<F::Output, Box<dyn Any + Send>>;

    #[expect(
        unsafe_code,
        reason = "structural pin projection of the future's only field, which is never moved (SAFETY comment on the block)"
    )]
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: structural pinning of the only field; it is never moved.
        let inner = unsafe { self.map_unchecked_mut(|s| &mut s.0) };
        match catch_unwind(AssertUnwindSafe(|| inner.poll(cx))) {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(v)) => Poll::Ready(Ok(v)),
            Err(payload) => Poll::Ready(Err(payload)),
        }
    }
}

/// Run `fut` to completion, turning a panic into `Err(payload)`. The
/// future is dropped (its guards run) before this returns.
pub async fn catch_panic<F: Future>(fut: F) -> Result<F::Output, Box<dyn Any + Send>> {
    CatchUnwind(fut).await
}

/// Spawn the loop `make()` builds, under supervision (see the module doc).
/// `make` is called again for every restart, so it must build a fresh loop
/// from its captured handles.
pub fn spawn_supervised<F, Fut>(name: &'static str, on_panic: OnPanic, make: F) -> JoinHandle<()>
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(supervise(name, on_panic, make))
}

/// The supervisor itself, for callers that spawn it their own way (a task
/// tracker, a `JoinSet`).
pub async fn supervise<F, Fut>(name: &'static str, on_panic: OnPanic, mut make: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ()>,
{
    let mut backoff = FIRST_BACKOFF;
    loop {
        let started = Instant::now();
        let Err(payload) = catch_panic(make()).await else {
            return;
        };
        note_task_panic(name);
        let message = super::panic::payload_text(payload.as_ref());
        match on_panic {
            OnPanic::Exit(code) => {
                tracing::error!(
                    task = name,
                    panic_message = %message,
                    exit_code = code,
                    "background task panicked and cannot be restarted in place; exiting so the process is replaced"
                );
                process::exit(code);
            }
            OnPanic::Restart => {
                if started.elapsed() >= HEALTHY_RUN {
                    backoff = FIRST_BACKOFF;
                }
                tracing::error!(
                    task = name,
                    panic_message = %message,
                    restart_in_ms = backoff.as_millis() as u64,
                    "background task panicked; restarting it"
                );
                time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[tokio::test(start_paused = true)]
    async fn a_panicking_loop_is_restarted_and_counted() {
        let runs = Arc::new(AtomicU32::new(0));
        let r = runs.clone();
        let handle = spawn_supervised("test_restart_loop", OnPanic::Restart, move || {
            let r = r.clone();
            async move {
                if r.fetch_add(1, Ordering::SeqCst) < 2 {
                    panic!("boom");
                }
            }
        });
        handle.await.unwrap();
        assert_eq!(runs.load(Ordering::SeqCst), 3);
        let count = restart_counts()
            .into_iter()
            .find(|(n, _)| *n == "test_restart_loop")
            .map(|(_, c)| c);
        assert_eq!(count, Some(2));
    }

    #[tokio::test]
    async fn a_loop_that_returns_is_not_restarted() {
        let runs = Arc::new(AtomicU32::new(0));
        let r = runs.clone();
        spawn_supervised("test_return_loop", OnPanic::Restart, move || {
            let r = r.clone();
            async move {
                r.fetch_add(1, Ordering::SeqCst);
            }
        })
        .await
        .unwrap();
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn aborting_the_supervisor_stops_the_loop() {
        let handle = spawn_supervised("test_abort_loop", OnPanic::Restart, || {
            future::pending::<()>()
        });
        handle.abort();
        assert!(handle.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn catch_panic_returns_the_payload() {
        let r = catch_panic(async { panic!("inner") }).await;
        let payload = r.unwrap_err();
        assert_eq!(super::super::panic::payload_text(payload.as_ref()), "inner");
    }
}
