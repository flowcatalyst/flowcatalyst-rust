//! In-place retry, whole-group release and worker panic safety — the pool
//! half of Go's `pool.go` (`drainGroup`, `runImmediate`, `retryOrRelease`,
//! `takeBuffered`), mirroring Go's `pool_retry_budget_test.go`,
//! `pool_release_test.go` and `pool_deferral_handback_test.go`.
//!
//! Every test runs on paused time, so multi-second backoffs cost nothing.

use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use fc_common::{
    BatchMessage, DispatchMode, MediationOutcome, MediationType, Message, MessageCallback,
    PoolConfig,
};
use fc_router::{Mediator, ProcessPool, MAX_IN_PIPELINE_ATTEMPTS};

#[derive(Debug, Clone, PartialEq)]
enum Event {
    Ack,
    Nack(Option<u32>),
    Dropped,
}

type Log = Arc<parking_lot::Mutex<Vec<(String, Event)>>>;

struct Callback {
    id: String,
    log: Log,
    honours: bool,
    settled: std::sync::atomic::AtomicBool,
}

impl Callback {
    fn record(&self, e: Event) {
        self.settled
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.log.lock().push((self.id.clone(), e));
    }
}

#[async_trait]
impl MessageCallback for Callback {
    async fn ack(&self) {
        self.record(Event::Ack);
    }
    async fn nack(&self, delay_seconds: Option<u32>) {
        self.record(Event::Nack(delay_seconds));
    }
    fn honours_delayed_return(&self) -> bool {
        self.honours
    }
}

impl Drop for Callback {
    fn drop(&mut self) {
        if !self.settled.load(std::sync::atomic::Ordering::SeqCst) {
            self.log.lock().push((self.id.clone(), Event::Dropped));
        }
    }
}

/// Answers each message id from its own script, then `Success` once the
/// script runs out. An id scripted with `None` panics.
struct Scripted {
    scripts: parking_lot::Mutex<HashMap<String, VecDeque<Option<MediationOutcome>>>>,
    seen: parking_lot::Mutex<Vec<(String, tokio::time::Instant)>>,
}

impl Scripted {
    fn new(scripts: Vec<(&str, Vec<Option<MediationOutcome>>)>) -> Arc<Self> {
        Arc::new(Self {
            scripts: parking_lot::Mutex::new(
                scripts
                    .into_iter()
                    .map(|(id, s)| (id.to_string(), s.into()))
                    .collect(),
            ),
            seen: parking_lot::Mutex::new(Vec::new()),
        })
    }

    fn seen(&self) -> Vec<String> {
        self.seen.lock().iter().map(|(id, _)| id.clone()).collect()
    }

    fn times(&self, id: &str) -> Vec<tokio::time::Instant> {
        self.seen
            .lock()
            .iter()
            .filter(|(m, _)| m == id)
            .map(|(_, t)| *t)
            .collect()
    }
}

#[async_trait]
impl Mediator for Scripted {
    async fn mediate(&self, message: &Message) -> MediationOutcome {
        self.seen
            .lock()
            .push((message.id.clone(), tokio::time::Instant::now()));
        let next = self
            .scripts
            .lock()
            .get_mut(&message.id)
            .and_then(|s| s.pop_front());
        match next {
            None => MediationOutcome::success(200),
            Some(Some(outcome)) => outcome,
            Some(None) => panic!("scripted panic for {}", message.id),
        }
    }
}

fn pool(mediator: Arc<Scripted>) -> Arc<ProcessPool> {
    Arc::new(ProcessPool::new(
        PoolConfig {
            code: "TEST".to_string(),
            concurrency: 4,
            rate_limit_per_minute: None,
        },
        mediator,
    ))
}

fn batch(
    id: &str,
    group: Option<&str>,
    mode: DispatchMode,
    batch_id: &str,
    log: &Log,
    honours: bool,
) -> BatchMessage {
    BatchMessage {
        message: Message {
            id: id.to_string(),
            pool_code: "TEST".to_string(),
            auth_token: None,
            signing_secret: None,
            mediation_type: MediationType::HTTP,
            mediation_target: "http://example.invalid/hook".to_string(),
            message_group_id: group.map(str::to_string),
            high_priority: false,
            dispatch_mode: mode,
            dispatch_mode_specified: true,
        },
        receipt_handle: format!("rh-{id}"),
        broker_message_id: None,
        queue_identifier: "q".to_string(),
        batch_id: Some(Arc::from(batch_id)),
        callback: Box::new(Callback {
            id: id.to_string(),
            log: log.clone(),
            honours,
            settled: std::sync::atomic::AtomicBool::new(false),
        }),
    }
}

fn ordered(id: &str, log: &Log) -> BatchMessage {
    batch(id, Some("g"), DispatchMode::NextOnError, "b1", log, true)
}

/// Wait (in paused time) until `n` broker events have been logged.
async fn settled(log: &Log, n: usize) -> Vec<(String, Event)> {
    for _ in 0..100_000 {
        if log.lock().len() >= n {
            return log.lock().clone();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("only {:?} settled, wanted {n}", log.lock());
}

fn deferred(delay: u32) -> Option<MediationOutcome> {
    Some(MediationOutcome::deferred(200, Some(delay)))
}

fn rate_limited(after: u32) -> Option<MediationOutcome> {
    Some(MediationOutcome::rate_limited(after))
}

/// H3: a 429 head is retried in place and nothing behind it is delivered
/// first; the group resumes in order once it succeeds.
#[tokio::test(start_paused = true)]
async fn rate_limited_head_is_retried_in_place_and_not_overtaken() {
    let mediator = Scripted::new(vec![("m1", vec![rate_limited(12), rate_limited(12)])]);
    let pool = pool(mediator.clone());
    pool.start().await;
    let log: Log = Default::default();
    pool.submit(ordered("m1", &log)).await.unwrap();
    pool.submit(ordered("m2", &log)).await.unwrap();

    let events = settled(&log, 2).await;
    assert_eq!(
        events,
        vec![("m1".into(), Event::Ack), ("m2".into(), Event::Ack)],
        "both delivered, head first, nothing nacked"
    );
    assert_eq!(mediator.seen(), vec!["m1", "m1", "m1", "m2"]);
    let t = mediator.times("m1");
    assert!(
        t[1] - t[0] >= Duration::from_secs(12),
        "Retry-After is the floor"
    );
    assert_eq!(pool.queue_size(), 0);
}

/// H1: `ack:false` with no delay is never a 0s release — it waits on the
/// deferred curve (5s, then 10s) in place.
#[tokio::test(start_paused = true)]
async fn deferral_without_delay_is_retried_on_the_deferred_curve() {
    let mediator = Scripted::new(vec![("m1", vec![deferred(0), deferred(0)])]);
    let pool = pool(mediator.clone());
    pool.start().await;
    let log: Log = Default::default();
    pool.submit(ordered("m1", &log)).await.unwrap();

    assert_eq!(settled(&log, 1).await, vec![("m1".into(), Event::Ack)]);
    let t = mediator.times("m1");
    assert_eq!(t.len(), 3);
    assert!(t[1] - t[0] >= Duration::from_secs(5));
    assert!(t[2] - t[1] >= Duration::from_secs(10));
}

/// The same retry, on the IMMEDIATE path.
#[tokio::test(start_paused = true)]
async fn immediate_message_is_retried_in_place() {
    let mediator = Scripted::new(vec![("m1", vec![deferred(0)])]);
    let pool = pool(mediator.clone());
    pool.start().await;
    let log: Log = Default::default();
    pool.submit(batch("m1", None, DispatchMode::Immediate, "b1", &log, true))
        .await
        .unwrap();

    assert_eq!(settled(&log, 1).await, vec![("m1".into(), Event::Ack)]);
    assert_eq!(mediator.seen(), vec!["m1", "m1"]);
    assert_eq!(pool.queue_size(), 0);
    assert_eq!(pool.active_workers(), 0);
}

/// R1: a deferral naming a delay, from a broker that holds it, goes back at
/// once with exactly that delay and takes its group along.
#[tokio::test(start_paused = true)]
async fn named_deferral_goes_back_to_a_broker_that_holds_it() {
    let mediator = Scripted::new(vec![("m1", vec![deferred(600)])]);
    let pool = pool(mediator.clone());
    pool.start().await;
    let log: Log = Default::default();
    pool.submit(ordered("m1", &log)).await.unwrap();
    pool.submit(ordered("m2", &log)).await.unwrap();

    let events = settled(&log, 2).await;
    assert_eq!(events[0], ("m1".into(), Event::Nack(Some(600))));
    assert!(matches!(events[1], (ref id, Event::Nack(_)) if id == "m2"));
    assert_eq!(
        mediator.seen(),
        vec!["m1"],
        "the sibling is never attempted"
    );
}

/// ...and is retried in place when the broker cannot hold a delay.
#[tokio::test(start_paused = true)]
async fn named_deferral_stays_in_place_when_the_broker_cannot_hold_it() {
    let mediator = Scripted::new(vec![("m1", vec![deferred(15)])]);
    let pool = pool(mediator.clone());
    pool.start().await;
    let log: Log = Default::default();
    pool.submit(batch(
        "m1",
        Some("g"),
        DispatchMode::NextOnError,
        "b1",
        &log,
        false,
    ))
    .await
    .unwrap();

    assert_eq!(settled(&log, 1).await, vec![("m1".into(), Event::Ack)]);
    let t = mediator.times("m1");
    assert!(t[1] - t[0] >= Duration::from_secs(15));
}

/// A target answering 429 for ever does not pin its group in memory: the
/// budget runs out and the whole group goes back to the broker.
#[tokio::test(start_paused = true)]
async fn spent_retry_budget_releases_the_whole_group() {
    let forever = (0..MAX_IN_PIPELINE_ATTEMPTS + 5)
        .map(|_| rate_limited(1))
        .collect();
    let mediator = Scripted::new(vec![("m1", forever)]);
    let pool = pool(mediator.clone());
    pool.start().await;
    let log: Log = Default::default();
    pool.submit(ordered("m1", &log)).await.unwrap();
    pool.submit(ordered("m2", &log)).await.unwrap();

    let events = settled(&log, 2).await;
    assert!(matches!(events[0], (ref id, Event::Nack(Some(_))) if id == "m1"));
    assert!(matches!(events[1], (ref id, Event::Nack(_)) if id == "m2"));
    assert_eq!(
        mediator.times("m1").len(),
        MAX_IN_PIPELINE_ATTEMPTS as usize
    );
    assert_eq!(mediator.times("m2").len(), 0);
    assert_eq!(pool.queue_size(), 0);
}

/// H3: a release hands back the group's whole buffer, not just the messages
/// that arrived in the head's batch.
#[tokio::test(start_paused = true)]
async fn release_takes_the_whole_group_buffer_across_batches() {
    let mediator = Scripted::new(vec![(
        "m1",
        vec![Some(MediationOutcome::error_connection("down".into()))],
    )]);
    let pool = pool(mediator.clone());
    pool.start().await;
    let log: Log = Default::default();
    pool.submit(batch(
        "m1",
        Some("g"),
        DispatchMode::NextOnError,
        "b1",
        &log,
        true,
    ))
    .await
    .unwrap();
    pool.submit(batch(
        "m2",
        Some("g"),
        DispatchMode::NextOnError,
        "b2",
        &log,
        true,
    ))
    .await
    .unwrap();

    let events = settled(&log, 2).await;
    assert_eq!(events[0], ("m1".into(), Event::Nack(Some(30))));
    // Held back no shorter than its head, so it cannot surface first on a
    // broker without group locks (delivery run 3, `platform-down`).
    assert_eq!(events[1], ("m2".into(), Event::Nack(Some(30))));
    assert_eq!(
        mediator.seen(),
        vec!["m1"],
        "m2 (another batch) is not delivered past it"
    );
}

/// A group is a strict FIFO: `high_priority` does not jump the queue inside
/// a group.
#[tokio::test(start_paused = true)]
async fn high_priority_does_not_reorder_a_group() {
    let mediator = Scripted::new(vec![]);
    let pool = pool(mediator.clone());
    pool.start().await;
    let log: Log = Default::default();
    // Nothing yields between these submits, so all three are buffered
    // before the drainer takes the first.
    pool.submit(ordered("m1", &log)).await.unwrap();
    pool.submit(ordered("m2", &log)).await.unwrap();
    let mut urgent = ordered("m3", &log);
    urgent.message.high_priority = true;
    pool.submit(urgent).await.unwrap();

    settled(&log, 3).await;
    assert_eq!(mediator.seen(), vec!["m1", "m2", "m3"]);
}

/// Shutdown does not wait out a retry backoff: the waiting message and its
/// group are handed back and the tracked tasks finish.
#[tokio::test(start_paused = true)]
async fn release_remainder_hands_back_a_group_waiting_to_retry() {
    let mediator = Scripted::new(vec![("m1", vec![rate_limited(240)])]);
    let pool = pool(mediator.clone());
    pool.start().await;
    let log: Log = Default::default();
    pool.submit(ordered("m1", &log)).await.unwrap();
    pool.submit(ordered("m2", &log)).await.unwrap();

    // Let m1 fail once and start its backoff.
    while mediator.seen().is_empty() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(10)).await;

    pool.drain().await;
    pool.release_remainder().await;
    tokio::time::timeout(Duration::from_secs(1), pool.wait_drained())
        .await
        .expect("no task waits out the 240s backoff");

    let events = log.lock().clone();
    assert_eq!(events.len(), 2, "{events:?}");
    assert!(events.iter().all(|(_, e)| matches!(e, Event::Nack(_))));
    assert_eq!(mediator.seen(), vec!["m1"]);
    assert_eq!(pool.queue_size(), 0);
}

/// A drain task that panics gives back every queue slot it held (the
/// message in hand and everything buffered behind it), its worker count and
/// its `mediating` entry; the abandoned messages are nacked as they drop.
#[tokio::test(start_paused = true)]
async fn panicking_drain_task_releases_its_slots() {
    let mediator = Scripted::new(vec![("m1", vec![None])]);
    let pool = pool(mediator.clone());
    pool.start().await;
    let log: Log = Default::default();
    pool.submit(ordered("m1", &log)).await.unwrap();
    pool.submit(ordered("m2", &log)).await.unwrap();
    pool.submit(ordered("m3", &log)).await.unwrap();

    let events = settled(&log, 3).await;
    assert!(
        events.iter().all(|(_, e)| *e == Event::Dropped),
        "{events:?}"
    );
    assert_eq!(pool.queue_size(), 0, "no slot leaked");
    assert_eq!(pool.active_workers(), 0);
    assert!(pool.mediating_snapshot().is_empty());

    // The group is usable again.
    pool.submit(ordered("m4", &log)).await.unwrap();
    let events = settled(&log, 4).await;
    assert_eq!(events[3], ("m4".into(), Event::Ack));
}

/// The IMMEDIATE path has the same guarantee.
#[tokio::test(start_paused = true)]
async fn panicking_immediate_task_releases_its_slot() {
    let mediator = Scripted::new(vec![("m1", vec![None])]);
    let pool = pool(mediator.clone());
    pool.start().await;
    let log: Log = Default::default();
    pool.submit(batch("m1", None, DispatchMode::Immediate, "b1", &log, true))
        .await
        .unwrap();

    assert_eq!(settled(&log, 1).await, vec![("m1".into(), Event::Dropped)]);
    assert_eq!(pool.queue_size(), 0);
    assert_eq!(pool.active_workers(), 0);
    assert!(pool.mediating_snapshot().is_empty());
}
