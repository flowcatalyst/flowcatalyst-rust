//! The host-wide executing budget: `FC_FN_MAX_EXECUTING` permits that every
//! runtime on the host shares (the WASM guests and the JS isolates alike),
//! so at most that many guests execute on a CPU at any moment, whichever
//! runtime they run on.
//!
//! **What counts as executing.** A guest holds a permit only while its
//! future is being *polled*: while guest code (and the host code it calls
//! synchronously) runs on a thread. A guest that awaits host I/O (an
//! outbound HTTP call, a database query, an emit, a timer) returns
//! `Pending`, and the permit goes back before the thread moves on; it takes
//! a permit again when it is woken, before it resumes. So one slow upstream
//! never holds the budget, and I/O still holds no thread.
//!
//! That boundary is where each runtime already gives up its thread:
//! - **WASM** ([`crate::wasm`]): a store runs on a wasmtime fiber, which
//!   suspends (and the invocation's future returns `Pending`) at every
//!   async host call that is not ready, at every epoch tick (1 ms) and at
//!   every fuel yield. A computing guest therefore gives its permit back
//!   once a millisecond and queues for the next one.
//! - **JS** (`fc-fnhost-js`): an isolate's future is one poll of the
//!   deno_core event loop per wake-up, which runs JavaScript until every
//!   task is waiting on an op or a timer. JavaScript is not preempted, so a
//!   computation holds its permit until its next `await` (or its deadline).
//!
//! **Fairness.** The permits are a FIFO semaphore: a released permit goes to
//! the guest that has waited longest, whichever runtime it is on, so
//! neither runtime can starve the other. A waiting guest's deadline still
//! applies: the WASM invoker stops a guest wherever it is at its deadline,
//! and [`Metered::until`] gives up the wait when the invocation is stopped.
//!
//! **Lanes.** A JS worker thread runs one isolate at a time, so an isolate
//! whose thread is busy with another cannot use a permit even if one is
//! free. Each worker is a [`Lane`]: its isolates take the lane before they
//! queue for a permit, so the permit queue only ever holds guests that can
//! run the moment they get one, and a permit never sits assigned to a
//! thread that is busy.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{ready, Context, Poll};

use tokio::sync::{AcquireError, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::{CancellationToken, WaitForCancellationFutureOwned};

type Acquiring = Pin<Box<dyn Future<Output = Result<OwnedSemaphorePermit, AcquireError>> + Send>>;

/// The shared permits (cheap to clone: every clone is the same budget).
#[derive(Clone)]
pub struct ExecBudget {
    inner: Arc<Inner>,
}

struct Inner {
    permits: Arc<Semaphore>,
    limit: usize,
    /// Guests holding a permit now.
    executing: AtomicUsize,
    /// Guests queued for a permit now.
    waiting: AtomicUsize,
    /// The most guests that have held a permit at once.
    peak: AtomicUsize,
}

impl fmt::Debug for ExecBudget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecBudget")
            .field("limit", &self.inner.limit)
            .field("executing", &self.executing())
            .field("waiting", &self.waiting())
            .finish()
    }
}

impl ExecBudget {
    /// `limit` permits (at least one).
    pub fn new(limit: usize) -> Self {
        let limit = limit.max(1);
        Self {
            inner: Arc::new(Inner {
                permits: Arc::new(Semaphore::new(limit)),
                limit,
                executing: AtomicUsize::new(0),
                waiting: AtomicUsize::new(0),
                peak: AtomicUsize::new(0),
            }),
        }
    }

    /// The number of permits: `FC_FN_MAX_EXECUTING`.
    pub fn limit(&self) -> usize {
        self.inner.limit
    }

    /// Guests executing now (permits in use).
    pub fn executing(&self) -> usize {
        self.inner.executing.load(Ordering::Relaxed)
    }

    /// Guests ready to run, queued for a permit now.
    pub fn waiting(&self) -> usize {
        self.inner.waiting.load(Ordering::Relaxed)
    }

    /// The most guests that have executed at once since the budget was
    /// made (or since [`ExecBudget::reset_peak`]). Never above
    /// [`ExecBudget::limit`].
    pub fn peak(&self) -> usize {
        self.inner.peak.load(Ordering::Relaxed)
    }

    /// Restarts [`ExecBudget::peak`] from the guests executing now; returns
    /// the peak it replaced.
    pub fn reset_peak(&self) -> usize {
        self.inner.peak.swap(self.executing(), Ordering::Relaxed)
    }

    /// `future`, holding a permit whenever it is polled and none between
    /// polls.
    pub fn run<F: Future>(&self, future: F) -> Metered<F> {
        Metered {
            admission: Admission::new(self.clone(), None),
            inner: Box::pin(future),
        }
    }

    /// [`ExecBudget::run`] on `lane`: the lane is taken before the permit,
    /// and both are held only while `future` is polled.
    pub fn run_on<F: Future>(&self, lane: &Lane, future: F) -> Metered<F> {
        Metered {
            admission: Admission::new(self.clone(), Some(lane.clone())),
            inner: Box::pin(future),
        }
    }
}

/// One thread's turn: whoever holds it is the one guest on that thread that
/// may run or queue for a permit (see the module docs).
#[derive(Clone)]
pub struct Lane(Arc<Semaphore>);

impl Lane {
    pub fn new() -> Self {
        Self(Arc::new(Semaphore::new(1)))
    }
}

impl Default for Lane {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Lane {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Lane")
    }
}

/// A permit's holder, for one poll: counts the guest as executing, and
/// gives the permit (and the lane) back when dropped.
struct Executing {
    inner: Arc<Inner>,
    _permit: OwnedSemaphorePermit,
    _lane: Option<OwnedSemaphorePermit>,
}

impl Executing {
    fn new(
        inner: Arc<Inner>,
        permit: OwnedSemaphorePermit,
        lane: Option<OwnedSemaphorePermit>,
    ) -> Self {
        let now = inner.executing.fetch_add(1, Ordering::Relaxed) + 1;
        inner.peak.fetch_max(now, Ordering::Relaxed);
        Self {
            inner,
            _permit: permit,
            _lane: lane,
        }
    }
}

impl Drop for Executing {
    fn drop(&mut self) {
        // Before the permit itself goes back (the fields drop after this),
        // so `executing` never counts more than `limit`.
        self.inner.executing.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Counts a queued guest in `waiting` for as long as it lives.
struct Queued(Arc<Inner>);

impl Queued {
    fn new(inner: Arc<Inner>) -> Self {
        inner.waiting.fetch_add(1, Ordering::Relaxed);
        Self(inner)
    }
}

impl Drop for Queued {
    fn drop(&mut self) {
        self.0.waiting.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Getting the lane (if any) and a permit for the next poll. A pending
/// acquisition keeps its place in the queue across polls; dropped, it gives
/// back whatever it was granted.
struct Admission {
    budget: ExecBudget,
    lane: Option<Lane>,
    lane_permit: Option<OwnedSemaphorePermit>,
    acquiring_lane: Option<Acquiring>,
    acquiring: Option<Acquiring>,
}

impl Admission {
    fn new(budget: ExecBudget, lane: Option<Lane>) -> Self {
        Self {
            budget,
            lane,
            lane_permit: None,
            acquiring_lane: None,
            acquiring: None,
        }
    }

    fn poll_admit(&mut self, cx: &mut Context<'_>) -> Poll<Executing> {
        if let Some(lane) = &self.lane {
            if self.lane_permit.is_none() {
                let permit = ready!(acquire(&lane.0, &mut self.acquiring_lane, None, cx));
                self.lane_permit = Some(permit);
            }
        }
        let inner = &self.budget.inner;
        let permit = ready!(acquire(
            &inner.permits,
            &mut self.acquiring,
            Some(inner),
            cx
        ));
        Poll::Ready(Executing::new(
            inner.clone(),
            permit,
            self.lane_permit.take(),
        ))
    }
}

/// One permit of `semaphore`: at once if one is free and nobody is queued
/// (tokio hands released permits to the queue first), else through `slot`,
/// which keeps the queue position. `counted` counts the wait in `waiting`.
fn acquire(
    semaphore: &Arc<Semaphore>,
    slot: &mut Option<Acquiring>,
    counted: Option<&Arc<Inner>>,
    cx: &mut Context<'_>,
) -> Poll<OwnedSemaphorePermit> {
    if slot.is_none() {
        if let Ok(permit) = semaphore.clone().try_acquire_owned() {
            return Poll::Ready(permit);
        }
        let queued = counted.map(|inner| Queued::new(inner.clone()));
        let acquire = semaphore.clone().acquire_owned();
        *slot = Some(Box::pin(async move {
            let permit = acquire.await;
            drop(queued);
            permit
        }));
    }
    let polled = slot.as_mut().expect("set just above").as_mut().poll(cx);
    match polled {
        Poll::Ready(permit) => {
            *slot = None;
            Poll::Ready(permit.expect("the budget's semaphores are never closed"))
        }
        Poll::Pending => Poll::Pending,
    }
}

/// A future that holds an executing permit only while it is polled
/// ([`ExecBudget::run`]).
pub struct Metered<F: Future> {
    admission: Admission,
    inner: Pin<Box<F>>,
}

impl<F: Future> Metered<F> {
    /// Gives up waiting for a permit once `stop` is cancelled: the output
    /// is then [`Stopped`], with the unfinished future, for the caller to
    /// answer (a timeout) and then drop. Only the wait is cut short: once
    /// polled, `future` sees `stop` itself.
    pub fn until(self, stop: CancellationToken) -> Until<F> {
        Until {
            metered: Some(self),
            stop: Box::pin(stop.cancelled_owned()),
        }
    }
}

impl<F: Future> Future for Metered<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        let this = self.get_mut();
        let executing = ready!(this.admission.poll_admit(cx));
        let polled = this.inner.as_mut().poll(cx);
        drop(executing);
        polled
    }
}

/// The unfinished future of an [`Until`] stopped while it waited for a
/// permit.
pub struct Stopped<F>(pub Pin<Box<F>>);

impl<F> fmt::Debug for Stopped<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Stopped")
    }
}

/// [`Metered::until`].
pub struct Until<F: Future> {
    metered: Option<Metered<F>>,
    stop: Pin<Box<WaitForCancellationFutureOwned>>,
}

impl<F: Future> Future for Until<F> {
    type Output = Result<F::Output, Stopped<F>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let metered = this.metered.as_mut().expect("polled after it finished");
        match metered.admission.poll_admit(cx) {
            Poll::Ready(executing) => {
                let polled = metered.inner.as_mut().poll(cx);
                drop(executing);
                match polled {
                    Poll::Ready(output) => {
                        this.metered = None;
                        Poll::Ready(Ok(output))
                    }
                    Poll::Pending => Poll::Pending,
                }
            }
            Poll::Pending => {
                if this.stop.as_mut().poll(cx).is_ready() {
                    let metered = this.metered.take().expect("checked above");
                    Poll::Ready(Err(Stopped(metered.inner)))
                } else {
                    Poll::Pending
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::{Duration, Instant};
    use tokio::time;

    /// Blocks its thread for `busy` inside one poll, then waits `idle`
    /// without holding anything, then blocks for `busy` again.
    async fn work(busy: Duration, idle: Duration) {
        thread::sleep(busy);
        time::sleep(idle).await;
        thread::sleep(busy);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn at_most_the_limit_execute_at_once() {
        let budget = ExecBudget::new(2);
        let tasks: Vec<_> = (0..8)
            .map(|_| tokio::spawn(budget.run(work(Duration::from_millis(30), Duration::ZERO))))
            .collect();
        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(budget.peak(), 2);
        assert_eq!(budget.executing(), 0);
        assert_eq!(budget.waiting(), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_guest_waiting_on_io_holds_no_permit() {
        let budget = ExecBudget::new(1);
        let sleeper = tokio::spawn(budget.run(work(Duration::ZERO, Duration::from_millis(400))));
        time::sleep(Duration::from_millis(50)).await;
        let started = Instant::now();
        budget
            .run(work(Duration::from_millis(10), Duration::ZERO))
            .await;
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "ran while the other slept: {:?}",
            started.elapsed()
        );
        sleeper.await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_stopped_wait_gives_the_future_back_without_running_it() {
        let budget = ExecBudget::new(1);
        let hog = tokio::spawn(budget.run(async { thread::sleep(Duration::from_millis(500)) }));
        time::sleep(Duration::from_millis(50)).await;
        let stop = CancellationToken::new();
        let ran = Arc::new(AtomicUsize::new(0));
        let waiter = {
            let ran = ran.clone();
            tokio::spawn(
                budget
                    .run(async move {
                        ran.fetch_add(1, Ordering::Relaxed);
                    })
                    .until(stop.clone()),
            )
        };
        time::sleep(Duration::from_millis(50)).await;
        assert_eq!(budget.waiting(), 1);
        let started = Instant::now();
        stop.cancel();
        assert!(waiter.await.unwrap().is_err(), "stopped while waiting");
        assert!(started.elapsed() < Duration::from_millis(200));
        assert_eq!(ran.load(Ordering::Relaxed), 0);
        assert_eq!(budget.waiting(), 0);
        hog.await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_lane_queues_its_guests_before_they_queue_for_a_permit() {
        let budget = ExecBudget::new(2);
        let lane = Lane::new();
        let first =
            tokio::spawn(budget.run_on(&lane, async { thread::sleep(Duration::from_millis(200)) }));
        time::sleep(Duration::from_millis(50)).await;
        let second = tokio::spawn(budget.run_on(&lane, async {}));
        time::sleep(Duration::from_millis(50)).await;
        // The second waits for its lane, not in the permit queue.
        assert_eq!((budget.executing(), budget.waiting()), (1, 0));
        first.await.unwrap();
        second.await.unwrap();
        assert_eq!(budget.peak(), 1);
    }
}
