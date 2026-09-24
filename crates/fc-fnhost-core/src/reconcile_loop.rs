//! Runs [`Reconciler::reconcile_once`] forever on one task until closed
//! (Java `fnhost/reconcile/ReconcileLoop.java`): 15 s between the end of one
//! run and the start of the next, [`ReconcileLoop::trigger`] wakes it early,
//! and any number of triggers during a run or a wait coalesce into exactly
//! one more run. A run that panics is logged and the loop continues.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use parking_lot::Mutex;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::clock::SharedClock;
use crate::reconciler::Reconciler;

/// Between the end of one run and the start of the next.
pub const INTERVAL: Duration = Duration::from_secs(15);

/// How long [`ReconcileLoop::close`] waits for a run to wind down.
const CLOSE_JOIN_TIMEOUT: Duration = Duration::from_secs(5);

pub struct ReconcileLoop {
    reconciler: Arc<Reconciler>,
    clock: SharedClock,
    interval: Duration,
    /// Holds at most one stored permit: that is the coalescing.
    trigger: Arc<Notify>,
    cancel: CancellationToken,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl ReconcileLoop {
    pub fn new(reconciler: Arc<Reconciler>, clock: SharedClock) -> Self {
        Self::with_interval(reconciler, clock, INTERVAL)
    }

    /// The interval is a constant in production; this exists so tests are
    /// not 15 s each.
    pub fn with_interval(
        reconciler: Arc<Reconciler>,
        clock: SharedClock,
        interval: Duration,
    ) -> Self {
        Self {
            reconciler,
            clock,
            interval,
            trigger: Arc::new(Notify::new()),
            cancel: CancellationToken::new(),
            handle: Mutex::new(None),
        }
    }

    /// Starts the loop's task. Call once.
    pub fn start(&self) {
        let reconciler = self.reconciler.clone();
        let clock = self.clock.clone();
        let interval = self.interval;
        let trigger = self.trigger.clone();
        let cancel = self.cancel.clone();
        let handle = tokio::spawn(async move {
            loop {
                let run = AssertUnwindSafe(reconciler.reconcile_once(clock.now())).catch_unwind();
                tokio::select! {
                    result = run => {
                        if result.is_err() {
                            tracing::warn!("reconcile run failed; continuing");
                        }
                    }
                    () = cancel.cancelled() => return,
                }
                tokio::select! {
                    () = tokio::time::sleep(interval) => {}
                    () = trigger.notified() => {}
                    () = cancel.cancelled() => return,
                }
            }
        });
        *self.handle.lock() = Some(handle);
    }

    /// Wakes a run early; coalesced (see the module docs).
    pub fn trigger(&self) {
        self.trigger.notify_one();
    }

    /// `/ready` and `/health` read this: a loop whose task has ended can
    /// never move desired state again.
    pub fn is_alive(&self) -> bool {
        self.handle
            .lock()
            .as_ref()
            .is_some_and(|h| !h.is_finished())
    }

    /// Stops the loop, cancelling a run blocked in the control plane, and
    /// waits (bounded) for it to end.
    pub async fn close(&self) {
        self.cancel.cancel();
        let handle = self.handle.lock().take();
        if let Some(mut handle) = handle {
            if tokio::time::timeout(CLOSE_JOIN_TIMEOUT, &mut handle)
                .await
                .is_err()
            {
                handle.abort();
            }
        }
    }
}
