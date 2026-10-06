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
//!
//! **Invariants.** What must always hold, and why:
//! - *A permit is held only inside `poll`.* [`Metered::poll`] and
//!   [`Until::poll`] get an `Executing` before they poll the guest and drop
//!   it before they return, whatever the guest returned. Between polls a
//!   guest holds no permit, so a guest parked on I/O can never starve the
//!   host, and `executing()` counts guests on a CPU, not guests in flight.
//! - *`executing() <= limit` at every instant.* `Executing` adds to the
//!   count only after its permit is granted and takes it off before the
//!   permit goes back (its `Drop` body runs before its fields drop), so the
//!   count never exceeds the permits handed out. `peak()` is taken from the
//!   same count, so it is bounded too.
//! - *A queue position survives a `Pending`.* A guest that finds no free
//!   permit keeps its `acquire_owned` future in `Admission` across polls,
//!   and tokio's semaphore is FIFO, so waking up and being polled again does
//!   not send it to the back. Dropping and re-creating that future on every
//!   poll would lose the place and let later arrivals overtake it.
//! - *The lane comes before the permit.* A lane-bound guest takes its lane
//!   first and holds it while it queues for a permit, and gives both back
//!   together at the end of the poll. So a permit is never granted to a
//!   guest whose thread is busy, and a lane never holds a permit between
//!   polls.
//! - *Dropping a [`Metered`] or an [`Until`] releases everything.* Every
//!   resource is owned by a value with a `Drop`: a granted permit by
//!   `Executing` (which never outlives the poll), a lane held while queued
//!   by `Admission`, a queued acquisition (and its `waiting` count, the
//!   `Queued` inside it) by its boxed future. So a guest dropped at any
//!   point (cancelled, timed out, stopped by [`Until`]) leaves no permit,
//!   lane or `waiting` count behind, and a permit tokio had already assigned
//!   to a dropped acquisition goes back to the semaphore.
//!
//! The stress test at the bottom of this file checks these under random
//! mixes of compute, I/O, cancellation and deadlines.

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
#[expect(
    clippy::expect_used,
    reason = "the slot was filled just above; the budget's semaphores are never closed while it lives"
)]
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

    #[expect(
        clippy::expect_used,
        reason = "the poll contract: a future is not polled again after it returns Ready; the same Option was checked Some earlier in this function"
    )]
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
    use std::hint;
    use std::sync::atomic::AtomicBool;
    use std::thread;
    use std::time::{Duration, Instant};
    use tokio::runtime::Builder;
    use tokio::{task, time};

    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};

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

    /// What a stress guest does between two awaits, or awaits.
    #[derive(Clone, Copy)]
    enum Step {
        /// Compute: keep the thread busy inside the poll.
        Spin(Duration),
        /// Give the thread up without waiting on anything.
        Yield,
        /// Simulated I/O: wait on a timer, holding nothing.
        Sleep(Duration),
    }

    /// How a stress guest ends.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Fate {
        /// Runs to the end: must complete.
        Completes,
        /// Behind [`Metered::until`] with a stop that never fires: must
        /// complete.
        UntilNever,
        /// Behind [`Metered::until`] with a stop fired after a deadline:
        /// completes, or is stopped while it waits.
        UntilDeadline(Duration),
        /// Its task is aborted after a while, wherever it is.
        Aborted(Duration),
        /// Dropped in place by a timeout, wherever it is.
        TimedOut(Duration),
    }

    impl Fate {
        fn must_complete(self) -> bool {
            matches!(self, Fate::Completes | Fate::UntilNever)
        }
    }

    /// The real concurrency, counted independently of the budget: how many
    /// guests are inside a poll now, overall and per lane.
    struct Probes {
        limit: usize,
        polling: AtomicUsize,
        lanes: Vec<AtomicUsize>,
        violations: AtomicUsize,
    }

    /// Polls `inner` counting itself in [`Probes`], and records a violation
    /// if more than `limit` guests, or more than one guest on its lane, are
    /// inside a poll at once, or if the budget does not count this guest.
    struct Probe<F> {
        inner: Pin<Box<F>>,
        probes: Arc<Probes>,
        lane: Option<usize>,
        budget: ExecBudget,
    }

    impl<F: Future> Future for Probe<F> {
        type Output = F::Output;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
            let this = self.get_mut();
            let probes = &this.probes;
            let polling = probes.polling.fetch_add(1, Ordering::SeqCst) + 1;
            let executing = this.budget.executing();
            if polling > probes.limit || executing == 0 || executing > probes.limit {
                probes.violations.fetch_add(1, Ordering::SeqCst);
            }
            if let Some(lane) = this.lane {
                if probes.lanes[lane].fetch_add(1, Ordering::SeqCst) != 0 {
                    probes.violations.fetch_add(1, Ordering::SeqCst);
                }
            }
            let polled = this.inner.as_mut().poll(cx);
            if let Some(lane) = this.lane {
                probes.lanes[lane].fetch_sub(1, Ordering::SeqCst);
            }
            probes.polling.fetch_sub(1, Ordering::SeqCst);
            polled
        }
    }

    async fn guest(steps: Vec<Step>, completed: Arc<AtomicUsize>) {
        for step in steps {
            match step {
                Step::Spin(busy) => {
                    let until = Instant::now() + busy;
                    while Instant::now() < until {
                        hint::spin_loop();
                    }
                }
                Step::Yield => task::yield_now().await,
                Step::Sleep(idle) => time::sleep(idle).await,
            }
        }
        completed.fetch_add(1, Ordering::SeqCst);
    }

    fn micros(rng: &mut StdRng, max: u64) -> Duration {
        Duration::from_micros(rng.random_range(0..=max))
    }

    fn plan(rng: &mut StdRng) -> (Vec<Step>, Fate) {
        let steps = (0..rng.random_range(1..=12))
            .map(|_| match rng.random_range(0..10) {
                0..=3 => Step::Spin(micros(rng, 300)),
                4..=5 => Step::Yield,
                _ => Step::Sleep(micros(rng, 3_000)),
            })
            .collect();
        let fate = match rng.random_range(0..10) {
            0..=3 => Fate::Completes,
            4..=5 => Fate::UntilNever,
            6..=7 => Fate::UntilDeadline(micros(rng, 20_000)),
            8 => Fate::Aborted(micros(rng, 20_000)),
            _ => Fate::TimedOut(micros(rng, 20_000)),
        };
        (steps, fate)
    }

    /// What one seed's run saw.
    #[derive(Default, Debug)]
    struct Seen {
        completed: usize,
        must_complete: usize,
        stopped: usize,
        cancelled: usize,
        max_waiting: usize,
        samples: usize,
    }

    /// One seed: `limit`, lanes and every guest's steps and fate come from
    /// the seed. Timing still varies from run to run, so the assertions are
    /// only the invariants, which hold under any interleaving.
    fn stress(seed: u64) -> Seen {
        const GUESTS: usize = 200;
        let mut rng = StdRng::seed_from_u64(seed);
        let limit = rng.random_range(1..=4);
        let lanes: Vec<Lane> = (0..rng.random_range(0..=3)).map(|_| Lane::new()).collect();
        let budget = ExecBudget::new(limit);
        let probes = Arc::new(Probes {
            limit,
            polling: AtomicUsize::new(0),
            lanes: lanes.iter().map(|_| AtomicUsize::new(0)).collect(),
            violations: AtomicUsize::new(0),
        });
        let completed = Arc::new(AtomicUsize::new(0));
        let plans: Vec<_> = (0..GUESTS)
            .map(|_| {
                let (steps, fate) = plan(&mut rng);
                let lane = (!lanes.is_empty() && rng.random_range(0..3) == 0)
                    .then(|| rng.random_range(0..lanes.len()));
                (steps, fate, lane)
            })
            .collect();

        // Samples the budget's counters from another thread for the whole
        // run: `executing() <= limit` must hold at every observation.
        let sampling = Arc::new(AtomicBool::new(true));
        let sampler = {
            let (budget, sampling) = (budget.clone(), sampling.clone());
            thread::spawn(move || {
                let (mut over, mut max_waiting, mut samples) = (0, 0, 0);
                while sampling.load(Ordering::SeqCst) {
                    if budget.executing() > budget.limit() {
                        over += 1;
                    }
                    max_waiting = max_waiting.max(budget.waiting());
                    samples += 1;
                    thread::yield_now();
                }
                (over, max_waiting, samples)
            })
        };

        // More threads than permits, so guests really do queue for them.
        let runtime = Builder::new_multi_thread()
            .worker_threads(6)
            .enable_all()
            .build()
            .unwrap();
        let mut seen = runtime.block_on(async {
            let handles: Vec<_> = plans
                .into_iter()
                .map(|(steps, fate, lane)| {
                    let probe = Probe {
                        inner: Box::pin(guest(steps, completed.clone())),
                        probes: probes.clone(),
                        lane,
                        budget: budget.clone(),
                    };
                    let metered = match lane {
                        Some(lane) => budget.run_on(&lanes[lane], probe),
                        None => budget.run(probe),
                    };
                    // `Some(stopped)` when an `Until` was stopped while it
                    // waited; `None` when it ended any other way.
                    let handle = match fate {
                        Fate::Completes | Fate::Aborted(_) => tokio::spawn(async move {
                            metered.await;
                            Some(false)
                        }),
                        Fate::UntilNever => tokio::spawn(async move {
                            Some(metered.until(CancellationToken::new()).await.is_err())
                        }),
                        Fate::UntilDeadline(deadline) => {
                            let stop = CancellationToken::new();
                            let timer = stop.clone();
                            tokio::spawn(async move {
                                time::sleep(deadline).await;
                                timer.cancel();
                            });
                            tokio::spawn(async move { Some(metered.until(stop).await.is_err()) })
                        }
                        Fate::TimedOut(after) => tokio::spawn(async move {
                            time::timeout(after, metered).await.ok().map(|()| false)
                        }),
                    };
                    if let Fate::Aborted(after) = fate {
                        let abort = handle.abort_handle();
                        tokio::spawn(async move {
                            time::sleep(after).await;
                            abort.abort();
                        });
                    }
                    (fate, handle)
                })
                .collect();

            let mut seen = Seen {
                must_complete: handles.iter().filter(|(f, _)| f.must_complete()).count(),
                ..Seen::default()
            };
            let joined = time::timeout(Duration::from_secs(60), async {
                for (fate, handle) in handles {
                    match handle.await {
                        Ok(Some(true)) => {
                            assert!(!fate.must_complete(), "seed {seed}: a {fate:?} was stopped");
                            seen.stopped += 1;
                        }
                        Ok(Some(false)) => seen.completed += 1,
                        Ok(None) | Err(_) => {
                            assert!(
                                !fate.must_complete(),
                                "seed {seed}: a {fate:?} was cancelled"
                            );
                            seen.cancelled += 1;
                        }
                    }
                }
            })
            .await;
            assert!(
                joined.is_ok(),
                "seed {seed}: guests still running after 60 s"
            );
            seen
        });
        drop(runtime);
        sampling.store(false, Ordering::SeqCst);
        let (over, max_waiting, samples) = sampler.join().unwrap();

        // Every guest whose task reported it finished ran to its end, and no
        // other did; every guest that had to complete did.
        assert_eq!(
            completed.load(Ordering::SeqCst),
            seen.completed,
            "seed {seed}"
        );
        assert!(
            seen.completed >= seen.must_complete,
            "seed {seed}: {seen:?}"
        );
        seen.max_waiting = max_waiting;
        seen.samples = samples;
        assert_eq!(
            over, 0,
            "seed {seed}: executing() above the limit {over} times"
        );
        assert_eq!(
            probes.violations.load(Ordering::SeqCst),
            0,
            "seed {seed}: concurrency"
        );
        assert!(
            budget.peak() <= limit,
            "seed {seed}: peak {}",
            budget.peak()
        );
        // Nothing leaked: every count back to 0, every permit and lane free.
        assert_eq!(budget.executing(), 0, "seed {seed}: executing leaked");
        assert_eq!(budget.waiting(), 0, "seed {seed}: waiting leaked");
        assert_eq!(
            budget.inner.permits.available_permits(),
            limit,
            "seed {seed}: permit leaked"
        );
        for lane in &lanes {
            assert_eq!(lane.0.available_permits(), 1, "seed {seed}: lane leaked");
        }
        seen
    }

    /// Many guests with random mixes of compute and I/O, random
    /// cancellations and [`Until`] deadlines, on several seeds (see the
    /// module's invariants). Every guest that is neither cancelled nor
    /// stopped must complete.
    #[test]
    fn random_guests_never_exceed_the_limit_or_leak_a_permit() {
        let mut stopped_or_cancelled = 0;
        let mut waited = 0;
        for seed in 0..12 {
            let seen = stress(seed);
            eprintln!("seed {seed}: {seen:?}");
            stopped_or_cancelled += seen.stopped + seen.cancelled;
            waited = waited.max(seen.max_waiting);
        }
        // Not invariants, but a run that never queued or cancelled anything
        // would not have tested much.
        assert!(waited > 0, "no guest ever queued for a permit");
        assert!(stopped_or_cancelled > 0, "no guest was ever cut short");
    }
}
