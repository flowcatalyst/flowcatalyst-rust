use async_trait::async_trait;
use aws_sdk_sqs::types::{
    ChangeMessageVisibilityBatchRequestEntry, DeleteMessageBatchRequestEntry,
};
use aws_sdk_sqs::{types::Message as SqsMessage, types::QueueAttributeName, Client};
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::mem;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use tokio::sync::{mpsc, oneshot, Mutex as AsyncMutex, Notify};
use tokio::time::{timeout, timeout_at, Instant as TokioInstant};
use tracing::{debug, error, info, warn};

use crate::{QueueConsumer, QueueError, QueueMetrics, RejectedLog, RejectedMessage, Result};
use aws_sdk_sqs::config::timeout::TimeoutConfig;
use aws_sdk_sqs::config::Builder;
use aws_sdk_sqs::error::DisplayErrorContext;
use aws_sdk_sqs::types::MessageSystemAttributeName;
use fc_common::{Message, QueuedMessage};
use std::result::Result as StdResult;
use std::time::Duration;
use std::time::Instant;

/// SQS's ceiling on a message's visibility timeout (12 hours, Go
/// `sqs.MaxVisibility`). A larger `ChangeMessageVisibility` is rejected, and
/// the rejected nack would leave the message to its natural timeout instead.
pub const SQS_MAX_VISIBILITY_SECONDS: u32 = 43_200;

/// A nack/defer delay as an SQS visibility timeout, clamped to
/// [`SQS_MAX_VISIBILITY_SECONDS`]. (Go clamps to what is left of the 12h
/// from the message's first receive; this is the flat ceiling.)
fn visibility_for(delay_seconds: Option<u32>) -> i32 {
    delay_seconds.unwrap_or(0).min(SQS_MAX_VISIBILITY_SECONDS) as i32
}

/// SQS's hard limit on entries in one `DeleteMessageBatch`.
const DELETE_BATCH_MAX: usize = 10;

/// Per-call SQS API timeout (matches the receive path).
const SQS_CALL_TIMEOUT: Duration = Duration::from_secs(25);

/// One entry's outcome from a batch call: `Err(reason)` if the broker reported
/// it failed.
type EntryOutcome = StdResult<(), String>;

/// Sends one `DeleteMessageBatch`. Whole-call failure is the outer `Err`;
/// otherwise one outcome per receipt, in order. A trait so the batcher is
/// testable without AWS.
#[async_trait]
trait DeleteBatchSender: Send + Sync + 'static {
    async fn send_batch(&self, receipts: &[String]) -> StdResult<Vec<EntryOutcome>, String>;
}

struct SqsDeleteSender {
    client: Client,
    queue_url: String,
}

#[async_trait]
impl DeleteBatchSender for SqsDeleteSender {
    async fn send_batch(&self, receipts: &[String]) -> StdResult<Vec<EntryOutcome>, String> {
        let mut entries = Vec::with_capacity(receipts.len());
        for (i, handle) in receipts.iter().enumerate() {
            let entry = DeleteMessageBatchRequestEntry::builder()
                .id(i.to_string())
                .receipt_handle(handle)
                .build()
                .map_err(|e| e.to_string())?;
            entries.push(entry);
        }
        let timeout_config = TimeoutConfig::builder()
            .operation_timeout(SQS_CALL_TIMEOUT)
            .build();
        let out = self
            .client
            .delete_message_batch()
            .queue_url(&self.queue_url)
            .set_entries(Some(entries))
            .customize()
            .config_override(Builder::default().timeout_config(timeout_config))
            .send()
            .await
            .map_err(|e| DisplayErrorContext(&e).to_string())?;

        let mut outcomes: Vec<EntryOutcome> = vec![Ok(()); receipts.len()];
        for f in out.failed() {
            let reason = format!("{}: {}", f.code(), f.message().unwrap_or("delete failed"));
            match f.id().parse::<usize>() {
                Ok(i) if i < outcomes.len() => outcomes[i] = Err(reason),
                _ => warn!(entry_id = %f.id(), "DeleteMessageBatch failure for unknown entry id"),
            }
        }
        Ok(outcomes)
    }
}

/// Runs once when a delete entry is finished with: its batch call completed
/// (success or failure), or the item was dropped unsent at shutdown.
type Completion = Box<dyn FnOnce() + Send>;

struct DeleteItem {
    receipt_handle: String,
    done: Option<oneshot::Sender<EntryOutcome>>,
    /// An ordered message's ack: the router waits for it before delivering the
    /// group's next message, so the batch must not linger for it.
    urgent: bool,
    on_complete: Option<Completion>,
}

impl DeleteItem {
    fn finish(mut self, outcome: EntryOutcome) {
        if let Some(done) = self.done.take() {
            let _ = done.send(outcome);
        }
        // `Drop` runs the completion.
    }
}

impl Drop for DeleteItem {
    fn drop(&mut self) {
        if let Some(on_complete) = self.on_complete.take() {
            on_complete();
        }
    }
}

/// Max extra drainers started under load, beside the one primary.
const DELETE_MAX_HELPERS: usize = 3;

/// The longest the primary drainer waits, from the first ack of a batch, for
/// more acks before sending a short batch. A full batch, and any batch holding
/// an urgent (ordered-message) ack, is sent at once.
const DELETE_LINGER: Duration = Duration::from_secs(5);

/// State shared by the batcher and its drainer tasks.
struct DeleteShared {
    queue: Mutex<VecDeque<DeleteItem>>,
    /// Wakes the primary drainer (`notify_one` stores a permit, so a push
    /// between its check and its wait is never lost).
    wake: Notify,
    closed: AtomicBool,
    helpers: AtomicUsize,
    sender: Arc<dyn DeleteBatchSender>,
    linger: Duration,
}

impl DeleteShared {
    /// Pop up to `DELETE_BATCH_MAX - batch.len()` items into `batch`.
    fn fill(&self, batch: &mut Vec<DeleteItem>) {
        let mut q = self.queue.lock();
        while batch.len() < DELETE_BATCH_MAX {
            match q.pop_front() {
                Some(item) => batch.push(item),
                None => break,
            }
        }
    }

    /// Shut down: dropping queued items fails their waiters.
    fn fail_queued(&self) {
        let items: Vec<DeleteItem> = self.queue.lock().drain(..).collect();
        drop(items);
    }

    async fn send(&self, batch: Vec<DeleteItem>) {
        let receipts: Vec<String> = batch.iter().map(|i| i.receipt_handle.clone()).collect();
        match self.sender.send_batch(&receipts).await {
            Ok(outcomes) => {
                for (i, item) in batch.into_iter().enumerate() {
                    let outcome = outcomes
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| Err("no result for batch entry".to_string()));
                    item.finish(outcome);
                }
            }
            Err(e) => {
                for item in batch {
                    item.finish(Err(e.clone()));
                }
            }
        }
    }

    /// The one long-lived drainer: waits for an ack, lingers up to `linger`
    /// for more (or until the batch is full), then sends.
    async fn primary(self: Arc<Self>) {
        loop {
            let mut batch: Vec<DeleteItem> = Vec::with_capacity(DELETE_BATCH_MAX);
            // Wait for the first item.
            loop {
                if self.closed.load(Ordering::SeqCst) {
                    self.fail_queued();
                    return;
                }
                self.fill(&mut batch);
                if !batch.is_empty() {
                    break;
                }
                self.wake.notified().await;
            }
            // Linger for more, measured from the first item.
            let deadline = TokioInstant::now() + self.linger;
            while batch.len() < DELETE_BATCH_MAX
                && !batch.iter().any(|i| i.urgent)
                && !self.closed.load(Ordering::SeqCst)
            {
                if timeout_at(deadline, self.wake.notified()).await.is_err() {
                    self.fill(&mut batch);
                    break;
                }
                self.fill(&mut batch);
            }
            if self.closed.load(Ordering::SeqCst) {
                drop(batch);
                self.fail_queued();
                return;
            }
            self.send(batch).await;
        }
    }

    /// A load-only drainer: takes what is already waiting, no linger, and
    /// exits when the queue is empty.
    async fn helper(self: Arc<Self>) {
        let _guard = HelperGuard(&self.helpers);
        loop {
            if self.closed.load(Ordering::SeqCst) {
                self.fail_queued();
                return;
            }
            let mut batch: Vec<DeleteItem> = Vec::with_capacity(DELETE_BATCH_MAX);
            self.fill(&mut batch);
            if batch.is_empty() {
                return;
            }
            self.send(batch).await;
        }
    }
}

/// Releases a helper slot however the helper task ends.
struct HelperGuard<'a>(&'a AtomicUsize);

impl Drop for HelperGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Coalesces concurrent acks into `DeleteMessageBatch` calls. One primary
/// drainer per queue lingers (up to `DELETE_LINGER`) to fill batches; under load (a full batch
/// already waiting) up to `DELETE_MAX_HELPERS` helpers drain in parallel.
struct DeleteBatcher {
    shared: Arc<DeleteShared>,
}

impl DeleteBatcher {
    /// Must be called inside a tokio runtime (spawns the primary drainer).
    fn start(sender: Arc<dyn DeleteBatchSender>) -> Self {
        Self::start_with_linger(sender, DELETE_LINGER)
    }

    fn start_with_linger(sender: Arc<dyn DeleteBatchSender>, linger: Duration) -> Self {
        let shared = Arc::new(DeleteShared {
            queue: Mutex::new(VecDeque::new()),
            wake: Notify::new(),
            closed: AtomicBool::new(false),
            helpers: AtomicUsize::new(0),
            sender,
            linger,
        });
        tokio::spawn(shared.clone().primary());
        Self { shared }
    }

    /// Start a helper if the queue holds a full batch and a slot is free.
    fn maybe_spawn_helper(&self, queued: usize) {
        if queued < DELETE_BATCH_MAX {
            return;
        }
        let claimed = self
            .shared
            .helpers
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < DELETE_MAX_HELPERS).then_some(n + 1)
            })
            .is_ok();
        if claimed {
            tokio::spawn(self.shared.clone().helper());
        }
    }

    /// Delete one receipt, resolving once the broker answered for it.
    #[cfg(test)]
    async fn delete(&self, receipt_handle: &str) -> EntryOutcome {
        self.delete_with(receipt_handle, false, None).await
    }

    /// [`delete`](Self::delete) with `urgent` (cut the batch being collected
    /// and send it now) and an `on_complete` hook the drainer runs when this
    /// entry's call has finished, even if the caller stopped waiting.
    async fn delete_with(
        &self,
        receipt_handle: &str,
        urgent: bool,
        on_complete: Option<Completion>,
    ) -> EntryOutcome {
        const SHUT: &str = "delete batcher is shut down";
        if self.shared.closed.load(Ordering::SeqCst) {
            return Err(SHUT.to_string());
        }
        let (done, wait) = oneshot::channel();
        let queued = {
            let mut q = self.shared.queue.lock();
            q.push_back(DeleteItem {
                receipt_handle: receipt_handle.to_string(),
                done: Some(done),
                urgent,
                on_complete,
            });
            q.len()
        };
        self.shared.wake.notify_one();
        if self.shared.closed.load(Ordering::SeqCst) {
            // Raced with shutdown after the drainers may have emptied the queue.
            self.shared.fail_queued();
        } else {
            self.maybe_spawn_helper(queued);
        }
        match wait.await {
            Ok(outcome) => outcome,
            Err(_) => Err("delete batcher shut down before the delete completed".to_string()),
        }
    }

    /// Stop accepting work; items still queued fail instead of hanging.
    fn shutdown(&self) {
        self.shared.closed.store(true, Ordering::SeqCst);
        self.shared.wake.notify_one();
    }
}

impl Drop for DeleteBatcher {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Bound on queued visibility changes per queue. When the broker cannot keep
/// up, `nack`/`defer` wait for room: deliberate back-pressure so a poll loop
/// cannot defer faster than SQS accepts.
const VISIBILITY_CAPACITY: usize = 4096;

/// SQS's hard limit on entries in one `ChangeMessageVisibilityBatch`.
const VISIBILITY_BATCH_MAX: usize = 10;

/// Drainer tasks per queue for visibility changes.
const VISIBILITY_DRAINERS: usize = 4;

/// A drainer exits after this long with nothing to send; the next change
/// starts the drainers again (so a dropped queue's tasks go away).
const VISIBILITY_IDLE_EXIT: Duration = Duration::from_secs(30);

/// One `ChangeMessageVisibility`, as a batch entry.
struct VisibilityChange {
    receipt_handle: String,
    visibility_seconds: i32,
}

/// Sends one `ChangeMessageVisibilityBatch`. Whole-call failure is the outer
/// `Err`; otherwise one outcome per change, in order. A trait so the batcher
/// is testable without AWS.
#[async_trait]
trait VisibilityBatchSender: Send + Sync + 'static {
    async fn send_batch(
        &self,
        changes: &[VisibilityChange],
    ) -> StdResult<Vec<EntryOutcome>, String>;
}

struct SqsVisibilitySender {
    client: Client,
    queue_url: String,
}

#[async_trait]
impl VisibilityBatchSender for SqsVisibilitySender {
    async fn send_batch(
        &self,
        changes: &[VisibilityChange],
    ) -> StdResult<Vec<EntryOutcome>, String> {
        let mut entries = Vec::with_capacity(changes.len());
        for (i, change) in changes.iter().enumerate() {
            let entry = ChangeMessageVisibilityBatchRequestEntry::builder()
                .id(i.to_string())
                .receipt_handle(&change.receipt_handle)
                .visibility_timeout(change.visibility_seconds)
                .build()
                .map_err(|e| e.to_string())?;
            entries.push(entry);
        }
        let timeout_config = TimeoutConfig::builder()
            .operation_timeout(SQS_CALL_TIMEOUT)
            .build();
        let out = self
            .client
            .change_message_visibility_batch()
            .queue_url(&self.queue_url)
            .set_entries(Some(entries))
            .customize()
            .config_override(Builder::default().timeout_config(timeout_config))
            .send()
            .await
            .map_err(|e| DisplayErrorContext(&e).to_string())?;

        let mut outcomes: Vec<EntryOutcome> = vec![Ok(()); changes.len()];
        for f in out.failed() {
            let reason = format!(
                "{}: {}",
                f.code(),
                f.message().unwrap_or("change visibility failed")
            );
            match f.id().parse::<usize>() {
                Ok(i) if i < outcomes.len() => outcomes[i] = Err(reason),
                _ => warn!(
                    entry_id = %f.id(),
                    "ChangeMessageVisibilityBatch failure for unknown entry id"
                ),
            }
        }
        Ok(outcomes)
    }
}

struct VisibilityItem {
    change: VisibilityChange,
    /// The caller's counter (nacked / deferred), bumped once the broker has
    /// answered or the call failed.
    settled: Arc<AtomicU64>,
}

/// State shared by the batcher and its drainers. Drainers hold this but never
/// the `Sender`, so dropping the batcher closes the channel and they finish
/// the queued items before ending.
struct VisibilityShared {
    rx: AsyncMutex<mpsc::Receiver<VisibilityItem>>,
    sender: Arc<dyn VisibilityBatchSender>,
    queue: String,
    idle: Duration,
    /// Drainers currently running.
    live: AtomicUsize,
    /// Entries the broker rejected, or that were in a call that failed.
    failed_entries: AtomicU64,
}

/// Sends nacks/defers as `ChangeMessageVisibilityBatch` calls of up to ten
/// without making the caller wait. No fill window: a drainer sends whatever
/// is already waiting.
struct VisibilityBatcher {
    tx: mpsc::Sender<VisibilityItem>,
    shared: Arc<VisibilityShared>,
}

impl VisibilityBatcher {
    fn new(
        sender: Arc<dyn VisibilityBatchSender>,
        queue: String,
        capacity: usize,
        idle: Duration,
    ) -> Self {
        let (tx, rx) = mpsc::channel(capacity);
        Self {
            tx,
            shared: Arc::new(VisibilityShared {
                rx: AsyncMutex::new(rx),
                sender,
                queue,
                idle,
                live: AtomicUsize::new(0),
                failed_entries: AtomicU64::new(0),
            }),
        }
    }

    /// Queue one change. Returns once it is queued; waits only while the
    /// channel is full. Must run inside a tokio runtime (starts the drainers).
    async fn submit(
        &self,
        receipt_handle: &str,
        visibility_seconds: i32,
        settled: &Arc<AtomicU64>,
    ) -> StdResult<(), String> {
        let item = VisibilityItem {
            change: VisibilityChange {
                receipt_handle: receipt_handle.to_string(),
                visibility_seconds,
            },
            settled: settled.clone(),
        };
        self.tx
            .send(item)
            .await
            .map_err(|_| "visibility batcher is shut down".to_string())?;
        VisibilityShared::start_drainers(&self.shared);
        Ok(())
    }
}

impl VisibilityShared {
    /// Start the drainers if none are running. Cheap when they are.
    fn start_drainers(this: &Arc<Self>) {
        if this
            .live
            .compare_exchange(0, VISIBILITY_DRAINERS, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        for _ in 0..VISIBILITY_DRAINERS {
            tokio::spawn(Self::drain(this.clone()));
        }
    }

    /// Take the next batch, or `None` when idle too long or the channel is
    /// closed and empty.
    async fn next_batch(&self) -> Option<Vec<VisibilityItem>> {
        let taken = timeout(self.idle, async {
            let mut rx = self.rx.lock().await;
            let first = rx.recv().await?;
            let mut batch = Vec::with_capacity(VISIBILITY_BATCH_MAX);
            batch.push(first);
            while batch.len() < VISIBILITY_BATCH_MAX {
                match rx.try_recv() {
                    Ok(item) => batch.push(item),
                    Err(_) => break,
                }
            }
            Some(batch)
        })
        .await;
        taken.unwrap_or(None)
    }

    async fn drain(this: Arc<Self>) {
        while let Some(batch) = this.next_batch().await {
            this.send(batch).await;
        }
        // Last one out restarts the drainers if a change slipped in between
        // this drainer's idle timeout and now (the submitter saw live > 0).
        if this.live.fetch_sub(1, Ordering::SeqCst) == 1 && !this.rx.lock().await.is_empty() {
            Self::start_drainers(&this);
        }
    }

    async fn send(&self, batch: Vec<VisibilityItem>) {
        let (changes, settlers): (Vec<_>, Vec<_>) =
            batch.into_iter().map(|i| (i.change, i.settled)).unzip();
        match self.sender.send_batch(&changes).await {
            Ok(outcomes) => {
                for (i, change) in changes.iter().enumerate() {
                    match outcomes.get(i) {
                        Some(Ok(())) => {}
                        other => {
                            self.failed_entries.fetch_add(1, Ordering::Relaxed);
                            let reason = match other {
                                Some(Err(r)) => r.as_str(),
                                _ => "no result for batch entry",
                            };
                            warn!(
                                queue = %self.queue,
                                receipt_handle = %change.receipt_handle,
                                error = %reason,
                                "ChangeMessageVisibility failed; message returns on its own visibility timeout"
                            );
                        }
                    }
                }
            }
            Err(e) => {
                self.failed_entries
                    .fetch_add(changes.len() as u64, Ordering::Relaxed);
                warn!(
                    queue = %self.queue,
                    entries = changes.len(),
                    error = %e,
                    "ChangeMessageVisibilityBatch failed; messages return on their own visibility timeout"
                );
            }
        }
        for counter in settlers {
            counter.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Acked-message-id guard. An id is remembered from the moment it is acked
/// until its `DeleteMessageBatch` entry has completed (success or failure),
/// plus `PENDING_DELETE_GRACE`. Entries still in flight are never pruned, so
/// memory is bounded by (acks in flight + the grace window's throughput).
///
/// Completions are pushed to a FIFO in completion order, so expiry pops from
/// the front only (one clock read per prune). Re-acking an id bumps its
/// generation; a completion record for an older generation is discarded
/// without touching the map (lazy invalidation).
#[derive(Default)]
struct PendingDeletes {
    /// id -> (generation, completion time; `None` while in flight).
    map: HashMap<Arc<str>, (u64, Option<Instant>)>,
    /// Completed entries in completion order.
    order: VecDeque<(Arc<str>, u64, Instant)>,
    next_gen: u64,
}

impl PendingDeletes {
    /// Remember `id` as in flight; returns the generation to hand to
    /// [`complete`](Self::complete).
    fn insert(&mut self, id: &str) -> u64 {
        self.next_gen += 1;
        self.map.insert(Arc::from(id), (self.next_gen, None));
        self.next_gen
    }

    /// The delete for generation `gen` of `id` finished at `at`; it expires
    /// `PENDING_DELETE_GRACE` later. A stale generation is ignored.
    fn complete(&mut self, id: &str, gen: u64, at: Instant) {
        let Some(key) = self.map.get_key_value(id).map(|(k, _)| Arc::clone(k)) else {
            return;
        };
        if let Some(entry) = self.map.get_mut(id) {
            if entry.0 == gen {
                entry.1 = Some(at);
                self.order.push_back((key, gen, at));
            }
        }
    }

    /// Drop completed entries whose completion is at least `grace` old at
    /// `now`. In-flight entries are never dropped.
    fn prune(&mut self, now: Instant, grace: Duration) {
        while let Some((_, _, at)) = self.order.front() {
            if now.saturating_duration_since(*at) < grace {
                break;
            }
            let Some((id, gen, _)) = self.order.pop_front() else {
                break;
            };
            if self.map.get(&id).is_some_and(|(g, _)| *g == gen) {
                self.map.remove(&id);
            }
        }
    }

    fn contains(&self, id: &str) -> bool {
        self.map.contains_key(id)
    }
}

/// Receipt handle -> (SQS message id, polled-at) with an insertion-ordered
/// FIFO so expiry pops from the front only (one clock read per prune) instead
/// of scanning the whole map on every poll.
///
/// Acks, nacks and defers remove handles from the map but cannot cheaply
/// remove them from the FIFO, so `prune` also discards stale entries (map
/// entry gone or re-inserted with a newer timestamp) from the front regardless
/// of age, and compacts the FIFO whenever it outgrows the live map by more
/// than 2x. The FIFO length is therefore bounded by O(live receipts).
#[derive(Default)]
struct ReceiptMap {
    map: HashMap<Arc<str>, (String, Instant)>,
    order: VecDeque<(Arc<str>, Instant)>,
}

impl ReceiptMap {
    /// Slack allowed in the FIFO beyond 2x the live map before compacting.
    const COMPACT_SLACK: usize = 64;

    fn insert(&mut self, handle: &str, msg_id: String, at: Instant) {
        let handle: Arc<str> = Arc::from(handle);
        self.map.insert(Arc::clone(&handle), (msg_id, at));
        self.order.push_back((handle, at));
    }

    fn remove(&mut self, handle: &str) -> Option<String> {
        self.map.remove(handle).map(|(id, _)| id)
    }

    fn is_live(&self, handle: &str, ts: Instant) -> bool {
        self.map.get(handle).is_some_and(|(_, t)| *t == ts)
    }

    /// Pop stale or expired entries from the FIFO front. A stale entry (already
    /// removed, or superseded by a re-insert) is popped immediately; a live
    /// entry is popped, and its map entry removed, only once age >= `ttl`.
    /// If stale entries are buried behind a live front entry, compact the FIFO
    /// when it exceeds 2x the live map (amortised O(1) per insert).
    ///
    /// Returns how many FIFO entries were examined (popped or discarded).
    fn prune(&mut self, now: Instant, ttl: Duration) -> usize {
        let mut examined = 0;
        while let Some((handle, ts)) = self.order.front() {
            let live = self.is_live(handle, *ts);
            if live && now.saturating_duration_since(*ts) < ttl {
                break;
            }
            let Some((handle, _)) = self.order.pop_front() else {
                break;
            };
            examined += 1;
            if live {
                self.map.remove(&handle);
            }
        }
        if self.order.len() > 2 * self.map.len() + Self::COMPACT_SLACK {
            let before = self.order.len();
            let mut order = mem::take(&mut self.order);
            order.retain(|(h, ts)| self.is_live(h, *ts));
            self.order = order;
            examined += before - self.order.len();
        }
        examined
    }
}

/// AWS SQS queue consumer
pub struct SqsQueueConsumer {
    client: Client,
    queue_url: String,
    queue_name: String,
    visibility_timeout_seconds: i32,
    wait_time_seconds: i32,
    running: AtomicBool,
    /// SQS message IDs that we've acked (successfully or not). SQS standard queues
    /// are at-least-once — even a successful DeleteMessage can be followed by a
    /// redelivery, and a failed delete obviously needs the same guard. Every
    /// redelivery while the ack is in flight, or within `PENDING_DELETE_GRACE`
    /// after its delete completed, is deleted without re-routing to the
    /// mediator. The drainer marks each entry's completion.
    pending_delete_ids: Arc<Mutex<PendingDeletes>>,
    /// Coalesces acks into DeleteMessageBatch; started on first ack.
    delete_batcher: OnceLock<DeleteBatcher>,
    /// Maps receipt handle -> SQS message ID so `ack` (which only receives the
    /// handle) can record the message ID in `pending_delete_ids`. Entries are
    /// pruned periodically to prevent unbounded growth.
    receipt_to_message_id: Mutex<ReceiptMap>,
    /// Total messages polled from queue
    total_polled: AtomicU64,
    /// Total messages successfully ACKed
    total_acked: AtomicU64,
    /// Total messages NACKed (actual failures)
    total_nacked: Arc<AtomicU64>,
    /// Total messages deferred (rate limiting, capacity - not failures)
    total_deferred: Arc<AtomicU64>,
    /// Batches nacks/defers into ChangeMessageVisibilityBatch; started on first use.
    visibility_batcher: OnceLock<VisibilityBatcher>,
    /// Messages deleted because they could not be decoded.
    rejected: RejectedLog,
}

impl SqsQueueConsumer {
    /// Default long poll wait time in seconds.
    /// 20 seconds matches TS version and minimises SQS API calls.
    /// AWS SQS max is 20 seconds.
    pub const DEFAULT_WAIT_TIME_SECONDS: i32 = 20;

    /// How long after its delete completed an acked SQS MessageId is still
    /// remembered, so a straggling redelivery is short-circuited to
    /// DeleteMessage without re-routing to the mediator.
    const PENDING_DELETE_GRACE: Duration = Duration::from_secs(5);

    /// How long a polled receipt handle stays in the handle -> MessageId map
    /// before an unacked entry is dropped (bounds the map for messages that
    /// are never acked, nacked or deferred).
    const RECEIPT_MAP_TTL: Duration = Duration::from_secs(15 * 60);

    /// Shared body of `ack` / `ack_urgent`; `urgent` makes the delete batcher
    /// send immediately instead of lingering for more acks.
    async fn ack_inner(&self, receipt_handle: &str, urgent: bool) -> Result<()> {
        // Always record the MessageId in pending_delete_ids — regardless of
        // whether DeleteMessage succeeds or fails. SQS standard queues are
        // at-least-once, so even a successful delete can be followed by a
        // redelivery; a failed delete obviously needs the same guard. The id
        // is remembered while the delete is in flight and for
        // `PENDING_DELETE_GRACE` after its batch entry completed; redeliveries
        // in that window are deleted in `poll` without being re-routed.
        let msg_id = self.receipt_to_message_id.lock().remove(receipt_handle);
        let on_complete: Option<Completion> = msg_id.as_ref().map(|id| {
            let gen = self.pending_delete_ids.lock().insert(id);
            let pending = Arc::clone(&self.pending_delete_ids);
            let id = id.clone();
            Box::new(move || pending.lock().complete(&id, gen, Instant::now())) as Completion
        });

        let batcher = self.delete_batcher.get_or_init(|| {
            DeleteBatcher::start(Arc::new(SqsDeleteSender {
                client: self.client.clone(),
                queue_url: self.queue_url.clone(),
            }))
        });

        match batcher
            .delete_with(receipt_handle, urgent, on_complete)
            .await
        {
            Ok(()) => {
                self.total_acked.fetch_add(1, Ordering::Relaxed);
                debug!(
                    receipt_handle = %receipt_handle,
                    queue = %self.queue_name,
                    "Message acknowledged in SQS"
                );
                Ok(())
            }
            Err(e) => {
                warn!(
                    queue = %self.queue_name,
                    message_id = ?msg_id,
                    error = %e,
                    "ACK failed — pending-delete guard will short-circuit redeliveries"
                );
                Err(QueueError::sqs(e))
            }
        }
    }

    /// Hand a nack/defer to the visibility batcher; returns once queued (waits
    /// only while the bounded channel is full). The broker's answer is only
    /// logged, and `counter` is bumped when it arrives.
    async fn enqueue_visibility(
        &self,
        receipt_handle: &str,
        visibility_timeout: i32,
        counter: &Arc<AtomicU64>,
    ) -> Result<()> {
        // The handle is spent the moment the change is queued: redelivery
        // arrives under a fresh handle that `poll` records, and `ack` is
        // never called for this one. Removing at enqueue (not on the broker's
        // answer) keeps the map from holding it while the batch is in flight.
        self.receipt_to_message_id.lock().remove(receipt_handle);
        let batcher = self.visibility_batcher.get_or_init(|| {
            VisibilityBatcher::new(
                Arc::new(SqsVisibilitySender {
                    client: self.client.clone(),
                    queue_url: self.queue_url.clone(),
                }),
                self.queue_name.clone(),
                VISIBILITY_CAPACITY,
                VISIBILITY_IDLE_EXIT,
            )
        });
        batcher
            .submit(receipt_handle, visibility_timeout, counter)
            .await
            .map_err(QueueError::sqs)
    }

    pub fn new(
        client: Client,
        queue_url: String,
        queue_name: String,
        visibility_timeout_seconds: i32,
    ) -> Self {
        Self {
            client,
            queue_url,
            queue_name,
            visibility_timeout_seconds,
            wait_time_seconds: Self::DEFAULT_WAIT_TIME_SECONDS,
            running: AtomicBool::new(true),
            pending_delete_ids: Arc::new(Mutex::new(PendingDeletes::default())),
            delete_batcher: OnceLock::new(),
            receipt_to_message_id: Mutex::new(ReceiptMap::default()),
            total_polled: AtomicU64::new(0),
            total_acked: AtomicU64::new(0),
            total_nacked: Arc::new(AtomicU64::new(0)),
            total_deferred: Arc::new(AtomicU64::new(0)),
            visibility_batcher: OnceLock::new(),
            rejected: RejectedLog::default(),
        }
    }

    /// Create from queue URL, extracting name
    pub async fn from_queue_url(
        client: Client,
        queue_url: String,
        visibility_timeout_seconds: i32,
    ) -> Self {
        let queue_name = queue_url
            .split('/')
            .next_back()
            .unwrap_or("unknown")
            .to_string();

        Self::new(client, queue_url, queue_name, visibility_timeout_seconds)
    }

    /// Set the long poll wait time in seconds (max 20).
    /// Shorter times mean faster shutdown response but more API calls.
    pub fn with_wait_time_seconds(mut self, seconds: i32) -> Self {
        self.wait_time_seconds = seconds.clamp(0, 20);
        self
    }

    fn parse_sqs_message(&self, sqs_msg: &SqsMessage) -> Result<(Message, String, Option<String>)> {
        let body = sqs_msg
            .body()
            .ok_or_else(|| QueueError::InvalidMessage("SQS message body is empty"))?;

        let message: Message = serde_json::from_str(body)?;

        let receipt_handle = sqs_msg
            .receipt_handle()
            .ok_or_else(|| QueueError::InvalidMessage("SQS message is missing its receipt handle"))?
            .to_string();

        let message_id = sqs_msg.message_id().map(|s| s.to_string());

        Ok((message, receipt_handle, message_id))
    }
}

#[async_trait]
impl QueueConsumer for SqsQueueConsumer {
    fn identifier(&self) -> &str {
        &self.queue_name
    }

    async fn poll(&self, max_messages: u32) -> Result<Vec<QueuedMessage>> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(QueueError::Stopped);
        }

        let max_per_poll = max_messages.min(10) as i32; // SQS max is 10

        // Java: 25s per-request API call timeout to prevent indefinite blocking
        let timeout_config = TimeoutConfig::builder()
            .operation_timeout(Duration::from_secs(25))
            .build();

        let result = self
            .client
            .receive_message()
            .queue_url(&self.queue_url)
            .max_number_of_messages(max_per_poll)
            .visibility_timeout(self.visibility_timeout_seconds)
            .wait_time_seconds(self.wait_time_seconds)
            .message_system_attribute_names(MessageSystemAttributeName::All)
            .message_attribute_names("All")
            .customize()
            .config_override(Builder::default().timeout_config(timeout_config))
            .send()
            .await
            .map_err(QueueError::sqs)?;

        let sqs_messages = result.messages.unwrap_or_default();
        let sqs_messages_count = sqs_messages.len();
        let mut messages = Vec::with_capacity(sqs_messages_count);

        for sqs_msg in sqs_messages {
            // If this MessageId is in pending-delete (we already acked it once),
            // delete the redelivery immediately and move on. The entry stays
            // until its grace after completion so every redelivery in the
            // window is short-circuited — not just the first one.
            if let Some(msg_id) = sqs_msg.message_id() {
                let should_delete = {
                    let mut pending = self.pending_delete_ids.lock();
                    pending.prune(Instant::now(), Self::PENDING_DELETE_GRACE);
                    pending.contains(msg_id)
                };
                if should_delete {
                    info!(
                        queue = %self.queue_name,
                        message_id = %msg_id,
                        "Redelivery of acked message — deleting immediately"
                    );
                    if let Some(handle) = sqs_msg.receipt_handle() {
                        // Delete directly; don't call self.ack() to avoid re-inserting
                        // into pending_delete_ids or racing with the tracking map.
                        let _ = self
                            .client
                            .delete_message()
                            .queue_url(&self.queue_url)
                            .receipt_handle(handle)
                            .send()
                            .await;
                    }
                    continue;
                }
            }

            match self.parse_sqs_message(&sqs_msg) {
                Ok((message, receipt_handle, broker_message_id)) => {
                    // Track receipt handle → message ID so `ack` can record the
                    // message ID in pending_delete_ids (ack only has the handle).
                    if let Some(ref msg_id) = broker_message_id {
                        let mut map = self.receipt_to_message_id.lock();
                        let now = Instant::now();
                        map.prune(now, Self::RECEIPT_MAP_TTL);
                        map.insert(&receipt_handle, msg_id.clone(), now);
                    }
                    messages.push(QueuedMessage {
                        message,
                        receipt_handle,
                        broker_message_id,
                        queue_identifier: self.queue_name.clone(),
                    });
                }
                Err(e) => {
                    error!(
                        queue = %self.queue_name,
                        error = %e,
                        "Failed to parse SQS message"
                    );
                    self.rejected
                        .record(sqs_msg.message_id().map(str::to_string), e.to_string());
                    // ACK the malformed message to prevent infinite retries
                    if let Some(handle) = sqs_msg.receipt_handle() {
                        let _ = self.ack(handle).await;
                    }
                }
            }
        }

        if !messages.is_empty() {
            self.total_polled
                .fetch_add(messages.len() as u64, Ordering::Relaxed);
            debug!(
                queue = %self.queue_name,
                count = messages.len(),
                "Polled messages from SQS"
            );
        }

        Ok(messages)
    }

    async fn ack(&self, receipt_handle: &str) -> Result<()> {
        self.ack_inner(receipt_handle, false).await
    }

    async fn ack_urgent(&self, receipt_handle: &str) -> Result<()> {
        self.ack_inner(receipt_handle, true).await
    }

    async fn nack(&self, receipt_handle: &str, delay_seconds: Option<u32>) -> Result<()> {
        // In SQS, NACK is done by setting visibility timeout to 0 (immediate retry)
        // or to a delay value for delayed retry
        let visibility_timeout = visibility_for(delay_seconds);
        self.enqueue_visibility(receipt_handle, visibility_timeout, &self.total_nacked)
            .await?;
        debug!(
            receipt_handle = %receipt_handle,
            queue = %self.queue_name,
            visibility_timeout = visibility_timeout,
            "Message NACK queued for SQS"
        );
        Ok(())
    }

    async fn defer(&self, receipt_handle: &str, delay_seconds: Option<u32>) -> Result<()> {
        // Same SQS operation as nack, but tracked separately as not a failure
        let visibility_timeout = visibility_for(delay_seconds);
        self.enqueue_visibility(receipt_handle, visibility_timeout, &self.total_deferred)
            .await?;
        debug!(
            receipt_handle = %receipt_handle,
            queue = %self.queue_name,
            visibility_timeout = visibility_timeout,
            "Message deferral queued for SQS (not counted as failure)"
        );
        Ok(())
    }

    async fn extend_visibility(&self, receipt_handle: &str, seconds: u32) -> Result<()> {
        self.client
            .change_message_visibility()
            .queue_url(&self.queue_url)
            .receipt_handle(receipt_handle)
            .visibility_timeout(seconds as i32)
            .send()
            .await
            .map_err(QueueError::sqs)?;

        debug!(
            receipt_handle = %receipt_handle,
            queue = %self.queue_name,
            seconds = seconds,
            "Visibility extended in SQS"
        );
        Ok(())
    }

    fn is_healthy(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        info!(queue = %self.queue_name, "SQS queue consumer stopped");
    }

    fn take_rejected(&self) -> Vec<RejectedMessage> {
        self.rejected.take()
    }

    fn get_counters(&self) -> Option<QueueMetrics> {
        Some(QueueMetrics {
            pending_messages: 0,   // Not available without SQS API call
            in_flight_messages: 0, // Not available without SQS API call
            queue_identifier: self.queue_name.clone(),
            total_polled: self.total_polled.load(Ordering::Relaxed),
            total_acked: self.total_acked.load(Ordering::Relaxed),
            total_nacked: self.total_nacked.load(Ordering::Relaxed),
            total_deferred: self.total_deferred.load(Ordering::Relaxed),
        })
    }

    async fn get_metrics(&self) -> Result<Option<QueueMetrics>> {
        let result = self
            .client
            .get_queue_attributes()
            .queue_url(&self.queue_url)
            .attribute_names(QueueAttributeName::ApproximateNumberOfMessages)
            .attribute_names(QueueAttributeName::ApproximateNumberOfMessagesNotVisible)
            .send()
            .await
            .map_err(QueueError::sqs)?;

        let attributes = result.attributes();

        let pending_messages = attributes
            .and_then(|attrs| attrs.get(&QueueAttributeName::ApproximateNumberOfMessages))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);

        let in_flight_messages = attributes
            .and_then(|attrs| attrs.get(&QueueAttributeName::ApproximateNumberOfMessagesNotVisible))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);

        debug!(
            queue = %self.queue_name,
            pending = pending_messages,
            in_flight = in_flight_messages,
            "Retrieved SQS queue metrics"
        );

        Ok(Some(QueueMetrics {
            pending_messages,
            in_flight_messages,
            queue_identifier: self.queue_name.clone(),
            total_polled: self.total_polled.load(Ordering::Relaxed),
            total_acked: self.total_acked.load(Ordering::Relaxed),
            total_nacked: self.total_nacked.load(Ordering::Relaxed),
            total_deferred: self.total_deferred.load(Ordering::Relaxed),
        }))
    }
}

#[cfg(test)]
mod visibility_clamp_tests {
    use super::*;

    #[test]
    fn nack_delays_are_clamped_to_the_sqs_ceiling() {
        assert_eq!(visibility_for(None), 0);
        assert_eq!(visibility_for(Some(30)), 30);
        assert_eq!(visibility_for(Some(43_200)), 43_200);
        assert_eq!(visibility_for(Some(100_000)), 43_200);
        assert_eq!(
            visibility_for(Some(u32::MAX)),
            43_200,
            "never wraps negative"
        );
    }
}

#[cfg(test)]
mod delete_batcher_tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use tokio::task::yield_now;
    use tokio::time::{sleep, timeout};

    /// Records each batch's size; behaviour set per test.
    struct FakeSender {
        sizes: Mutex<Vec<usize>>,
        calls: AtomicUsize,
        /// Receipts that report a per-entry failure.
        fail_receipts: Vec<String>,
        whole_call_error: bool,
        /// Hold each call this long so concurrent acks pile up.
        delay: Duration,
        in_flight: AtomicUsize,
        max_in_flight: AtomicUsize,
    }

    impl FakeSender {
        fn new() -> Self {
            Self {
                in_flight: AtomicUsize::new(0),
                max_in_flight: AtomicUsize::new(0),
                sizes: Mutex::new(Vec::new()),
                calls: AtomicUsize::new(0),
                fail_receipts: Vec::new(),
                whole_call_error: false,
                delay: Duration::ZERO,
            }
        }
    }

    #[async_trait]
    impl DeleteBatchSender for FakeSender {
        async fn send_batch(&self, receipts: &[String]) -> StdResult<Vec<EntryOutcome>, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.sizes.lock().push(receipts.len());
            let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_in_flight.fetch_max(now, Ordering::SeqCst);
            if !self.delay.is_zero() {
                sleep(self.delay).await;
            }
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            if self.whole_call_error {
                return Err("boom".to_string());
            }
            Ok(receipts
                .iter()
                .map(|r| {
                    if self.fail_receipts.contains(r) {
                        Err("entry failed".to_string())
                    } else {
                        Ok(())
                    }
                })
                .collect())
        }
    }

    #[tokio::test]
    async fn batches_never_exceed_ten_and_coalesce() {
        let fake = Arc::new(FakeSender {
            delay: Duration::from_millis(50),
            ..FakeSender::new()
        });
        let batcher = Arc::new(DeleteBatcher::start_with_linger(
            fake.clone(),
            Duration::from_millis(1),
        ));
        let mut tasks = Vec::new();
        for i in 0..200 {
            let b = batcher.clone();
            tasks.push(tokio::spawn(
                async move { b.delete(&format!("r{i}")).await },
            ));
        }
        for t in tasks {
            assert_eq!(t.await.unwrap(), Ok(()));
        }
        let sizes = fake.sizes.lock().clone();
        assert!(sizes.iter().all(|&n| (1..=10).contains(&n)), "{sizes:?}");
        assert_eq!(sizes.iter().sum::<usize>(), 200);
        assert!(
            fake.calls.load(Ordering::SeqCst) < 200,
            "acks must coalesce, got {} calls",
            fake.calls.load(Ordering::SeqCst)
        );
    }

    #[tokio::test]
    async fn per_entry_failure_fails_only_that_ack() {
        let fake = Arc::new(FakeSender {
            fail_receipts: vec!["bad".to_string()],
            delay: Duration::from_millis(30),
            ..FakeSender::new()
        });
        let batcher = Arc::new(DeleteBatcher::start_with_linger(
            fake,
            Duration::from_millis(1),
        ));
        let mut tasks = Vec::new();
        for name in ["a", "bad", "b", "c"] {
            let b = batcher.clone();
            tasks.push((name, tokio::spawn(async move { b.delete(name).await })));
        }
        for (name, t) in tasks {
            let out = t.await.unwrap();
            if name == "bad" {
                assert!(out.is_err());
            } else {
                assert_eq!(out, Ok(()), "{name}");
            }
        }
    }

    #[tokio::test]
    async fn whole_call_error_fails_every_entry() {
        let fake = Arc::new(FakeSender {
            whole_call_error: true,
            delay: Duration::from_millis(30),
            ..FakeSender::new()
        });
        let batcher = Arc::new(DeleteBatcher::start_with_linger(
            fake,
            Duration::from_millis(1),
        ));
        let mut tasks = Vec::new();
        for i in 0..5 {
            let b = batcher.clone();
            tasks.push(tokio::spawn(
                async move { b.delete(&format!("r{i}")).await },
            ));
        }
        for t in tasks {
            assert!(t.await.unwrap().is_err());
        }
    }

    #[tokio::test]
    async fn shutdown_fails_waiting_items_without_hanging() {
        // Every drainer is stuck in a long call; further items sit queued.
        let fake = Arc::new(FakeSender {
            delay: Duration::from_millis(300),
            ..FakeSender::new()
        });
        let batcher = Arc::new(DeleteBatcher::start_with_linger(
            fake,
            Duration::from_millis(1),
        ));
        let mut tasks = Vec::new();
        for i in 0..((DELETE_MAX_HELPERS + 1) * DELETE_BATCH_MAX + 20) {
            let b = batcher.clone();
            tasks.push(tokio::spawn(
                async move { b.delete(&format!("r{i}")).await },
            ));
            yield_now().await;
        }
        sleep(Duration::from_millis(50)).await;
        batcher.shutdown();
        let all = timeout(Duration::from_secs(5), async {
            let mut failed = 0;
            for t in tasks {
                if t.await.unwrap().is_err() {
                    failed += 1;
                }
            }
            failed
        })
        .await
        .expect("waiting acks must not hang after shutdown");
        assert!(all > 0, "queued items must fail on shutdown");
        assert!(batcher.delete("late").await.is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn steady_moderate_load_fills_batches() {
        // tokio timers have 1ms resolution, so this is the 1ms-linger /
        // 100us-spacing scenario scaled 10x: one ack per 1ms, linger 10ms
        // (same acks-per-linger ratio).
        let fake = Arc::new(FakeSender {
            delay: Duration::from_millis(2),
            ..FakeSender::new()
        });
        let batcher = Arc::new(DeleteBatcher::start_with_linger(
            fake.clone(),
            Duration::from_millis(10),
        ));
        let mut tasks = Vec::new();
        for i in 0..2000 {
            let b = batcher.clone();
            tasks.push(tokio::spawn(
                async move { b.delete(&format!("r{i}")).await },
            ));
            sleep(Duration::from_millis(1)).await;
        }
        for t in tasks {
            assert_eq!(t.await.unwrap(), Ok(()));
        }
        let sizes = fake.sizes.lock().clone();
        assert_eq!(sizes.iter().sum::<usize>(), 2000);
        assert!(sizes.iter().all(|&n| (1..=10).contains(&n)), "{sizes:?}");
        let avg = 2000.0 / sizes.len() as f64;
        assert!(avg >= 8.0, "average batch {avg} over {} calls", sizes.len());
    }

    #[tokio::test(start_paused = true)]
    async fn lone_ack_completes_after_about_one_linger() {
        let linger = DELETE_LINGER;
        let fake = Arc::new(FakeSender::new());
        let batcher = DeleteBatcher::start_with_linger(fake.clone(), linger);
        let start = TokioInstant::now();
        assert_eq!(batcher.delete("only").await, Ok(()));
        let elapsed = start.elapsed();
        assert!(elapsed >= linger, "sent before the linger: {elapsed:?}");
        assert!(elapsed < linger * 2, "stuck past the linger: {elapsed:?}");
        assert_eq!(*fake.sizes.lock(), vec![1]);
    }

    #[test]
    fn default_linger_cap_is_five_seconds() {
        assert_eq!(DELETE_LINGER, Duration::from_secs(5));
    }

    #[tokio::test(start_paused = true)]
    async fn urgent_ack_cuts_the_batch_immediately_with_earlier_acks() {
        let linger = Duration::from_secs(3600);
        let fake = Arc::new(FakeSender::new());
        let batcher = Arc::new(DeleteBatcher::start_with_linger(fake.clone(), linger));
        let start = TokioInstant::now();
        let mut tasks = Vec::new();
        for i in 0..3 {
            let b = batcher.clone();
            tasks.push(tokio::spawn(
                async move { b.delete(&format!("r{i}")).await },
            ));
        }
        sleep(Duration::from_millis(10)).await;
        assert!(fake.sizes.lock().is_empty(), "non-urgent acks must linger");
        let b = batcher.clone();
        tasks.push(tokio::spawn(async move {
            b.delete_with("urgent", true, None).await
        }));
        for t in tasks {
            assert_eq!(t.await.unwrap(), Ok(()));
        }
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "urgent must not linger"
        );
        assert_eq!(*fake.sizes.lock(), vec![4]);
    }

    #[tokio::test(start_paused = true)]
    async fn completion_hook_runs_for_success_and_failure() {
        let fake = Arc::new(FakeSender {
            fail_receipts: vec!["bad".to_string()],
            ..FakeSender::new()
        });
        let batcher = Arc::new(DeleteBatcher::start_with_linger(
            fake,
            Duration::from_millis(5),
        ));
        let done = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for name in ["ok", "bad"] {
            let b = batcher.clone();
            let d = done.clone();
            tasks.push(tokio::spawn(async move {
                b.delete_with(
                    name,
                    false,
                    Some(Box::new(move || {
                        d.fetch_add(1, Ordering::SeqCst);
                    })),
                )
                .await
            }));
        }
        for t in tasks {
            let _ = t.await.unwrap();
        }
        assert_eq!(done.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn full_batch_is_sent_without_lingering() {
        let linger = Duration::from_secs(3600);
        let fake = Arc::new(FakeSender::new());
        let batcher = Arc::new(DeleteBatcher::start_with_linger(fake.clone(), linger));
        let start = TokioInstant::now();
        let mut tasks = Vec::new();
        for i in 0..DELETE_BATCH_MAX {
            let b = batcher.clone();
            tasks.push(tokio::spawn(
                async move { b.delete(&format!("r{i}")).await },
            ));
        }
        for t in tasks {
            assert_eq!(t.await.unwrap(), Ok(()));
        }
        assert!(start.elapsed() < linger, "a full batch must not linger");
        assert_eq!(*fake.sizes.lock(), vec![DELETE_BATCH_MAX]);
    }

    #[tokio::test(start_paused = true)]
    async fn burst_engages_helpers_and_they_exit_when_idle() {
        let fake = Arc::new(FakeSender {
            delay: Duration::from_millis(20),
            ..FakeSender::new()
        });
        let batcher = Arc::new(DeleteBatcher::start_with_linger(
            fake.clone(),
            Duration::from_millis(1),
        ));
        let mut tasks = Vec::new();
        for i in 0..500 {
            let b = batcher.clone();
            tasks.push(tokio::spawn(
                async move { b.delete(&format!("r{i}")).await },
            ));
        }
        for t in tasks {
            assert_eq!(t.await.unwrap(), Ok(()));
        }
        let sizes = fake.sizes.lock().clone();
        assert!(sizes.iter().all(|&n| (1..=10).contains(&n)), "{sizes:?}");
        assert_eq!(sizes.iter().sum::<usize>(), 500);
        let peak = fake.max_in_flight.load(Ordering::SeqCst);
        assert!(peak >= 2, "helpers must run concurrent calls, peak {peak}");
        assert!(
            peak <= DELETE_MAX_HELPERS + 1,
            "helper cap exceeded, peak {peak}"
        );
        sleep(Duration::from_millis(100)).await;
        assert_eq!(
            batcher.shared.helpers.load(Ordering::SeqCst),
            0,
            "helpers must exit when idle"
        );
    }
}

#[cfg(test)]
mod visibility_batcher_tests {
    use super::*;
    use tokio::sync::Semaphore;
    use tokio::time::{sleep, timeout};

    /// Records each batch; calls block on `gate` until permits are added
    /// (starts open unless `gated`).
    struct FakeSender {
        batches: Mutex<Vec<Vec<(String, i32)>>>,
        gate: Semaphore,
        gated: bool,
        fail_receipts: Vec<String>,
        whole_call_error: bool,
    }

    impl FakeSender {
        fn new() -> Self {
            Self {
                batches: Mutex::new(Vec::new()),
                gate: Semaphore::new(0),
                gated: false,
                fail_receipts: Vec::new(),
                whole_call_error: false,
            }
        }
        fn gated() -> Self {
            Self {
                gated: true,
                ..Self::new()
            }
        }
        fn open(&self) {
            self.gate.add_permits(1_000_000);
        }
        fn sizes(&self) -> Vec<usize> {
            self.batches.lock().iter().map(Vec::len).collect()
        }
    }

    #[async_trait]
    impl VisibilityBatchSender for FakeSender {
        async fn send_batch(
            &self,
            changes: &[VisibilityChange],
        ) -> StdResult<Vec<EntryOutcome>, String> {
            self.batches.lock().push(
                changes
                    .iter()
                    .map(|c| (c.receipt_handle.clone(), c.visibility_seconds))
                    .collect(),
            );
            if self.gated {
                self.gate.acquire().await.unwrap().forget();
            } else {
                // Let concurrent submits pile up behind a call.
                sleep(Duration::from_millis(20)).await;
            }
            if self.whole_call_error {
                return Err("boom".to_string());
            }
            Ok(changes
                .iter()
                .map(|c| {
                    if self.fail_receipts.contains(&c.receipt_handle) {
                        Err("entry failed".to_string())
                    } else {
                        Ok(())
                    }
                })
                .collect())
        }
    }

    fn batcher(fake: &Arc<FakeSender>, capacity: usize, idle: Duration) -> VisibilityBatcher {
        VisibilityBatcher::new(fake.clone(), "q".to_string(), capacity, idle)
    }

    async fn until(what: &str, mut cond: impl FnMut() -> bool) {
        timeout(Duration::from_secs(5), async {
            while !cond() {
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
    }

    #[tokio::test]
    async fn batches_never_exceed_ten_and_keep_each_timeout() {
        let fake = Arc::new(FakeSender::new());
        let b = batcher(&fake, VISIBILITY_CAPACITY, VISIBILITY_IDLE_EXIT);
        let counter = Arc::new(AtomicU64::new(0));
        for i in 0..200 {
            b.submit(&format!("r{i}"), i, &counter).await.unwrap();
        }
        until("all settled", || counter.load(Ordering::Relaxed) == 200).await;
        let sizes = fake.sizes();
        assert!(sizes.iter().all(|&n| (1..=10).contains(&n)), "{sizes:?}");
        assert_eq!(sizes.iter().sum::<usize>(), 200);
        assert!(sizes.len() < 200, "changes must coalesce: {sizes:?}");
        for batch in fake.batches.lock().iter() {
            for (handle, secs) in batch {
                assert_eq!(*handle, format!("r{secs}"), "entry keeps its own timeout");
            }
        }
    }

    #[tokio::test]
    async fn submit_returns_before_the_sender_answers() {
        let fake = Arc::new(FakeSender::gated());
        let b = batcher(&fake, VISIBILITY_CAPACITY, VISIBILITY_IDLE_EXIT);
        let counter = Arc::new(AtomicU64::new(0));
        for i in 0..5 {
            timeout(
                Duration::from_millis(500),
                b.submit(&format!("r{i}"), 1, &counter),
            )
            .await
            .expect("submit must not wait for the broker")
            .unwrap();
        }
        sleep(Duration::from_millis(50)).await;
        assert_eq!(
            counter.load(Ordering::Relaxed),
            0,
            "broker has not answered"
        );
        fake.open();
        until("settled", || counter.load(Ordering::Relaxed) == 5).await;
    }

    #[tokio::test]
    async fn whole_call_failure_still_counts_and_is_recorded() {
        let fake = Arc::new(FakeSender {
            whole_call_error: true,
            ..FakeSender::new()
        });
        let b = batcher(&fake, VISIBILITY_CAPACITY, VISIBILITY_IDLE_EXIT);
        let counter = Arc::new(AtomicU64::new(0));
        for i in 0..5 {
            b.submit(&format!("r{i}"), 1, &counter).await.unwrap();
        }
        until("settled", || counter.load(Ordering::Relaxed) == 5).await;
        assert_eq!(b.shared.failed_entries.load(Ordering::Relaxed), 5);
    }

    #[tokio::test]
    async fn per_entry_failure_is_reported_and_still_counts() {
        let fake = Arc::new(FakeSender {
            fail_receipts: vec!["bad".to_string()],
            ..FakeSender::new()
        });
        let b = batcher(&fake, VISIBILITY_CAPACITY, VISIBILITY_IDLE_EXIT);
        let counter = Arc::new(AtomicU64::new(0));
        for name in ["a", "bad", "b"] {
            b.submit(name, 1, &counter).await.unwrap();
        }
        until("settled", || counter.load(Ordering::Relaxed) == 3).await;
        assert_eq!(b.shared.failed_entries.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn full_channel_makes_submit_wait_until_drained() {
        let fake = Arc::new(FakeSender::gated());
        let b = Arc::new(batcher(&fake, 4, VISIBILITY_IDLE_EXIT));
        let counter = Arc::new(AtomicU64::new(0));
        // One item per drainer, each stuck in the gated call.
        for i in 0..VISIBILITY_DRAINERS {
            b.submit(&format!("d{i}"), 1, &counter).await.unwrap();
            until("drainer took it", || fake.batches.lock().len() == i + 1).await;
        }
        // Fill the channel.
        for i in 0..4 {
            b.submit(&format!("f{i}"), 1, &counter).await.unwrap();
        }
        let b2 = b.clone();
        let c2 = counter.clone();
        let mut blocked = tokio::spawn(async move { b2.submit("over", 1, &c2).await });
        assert!(
            timeout(Duration::from_millis(150), &mut blocked)
                .await
                .is_err(),
            "submit into a full channel must wait"
        );
        fake.open();
        timeout(Duration::from_secs(5), blocked)
            .await
            .expect("waiting submit completes once drained")
            .unwrap()
            .unwrap();
        until("all settled", || counter.load(Ordering::Relaxed) == 9).await;
    }

    #[tokio::test]
    async fn drainers_exit_when_idle_and_restart_on_next_use() {
        let fake = Arc::new(FakeSender::new());
        let b = batcher(&fake, VISIBILITY_CAPACITY, Duration::from_millis(100));
        let counter = Arc::new(AtomicU64::new(0));
        assert_eq!(b.shared.live.load(Ordering::SeqCst), 0, "lazy start");
        b.submit("one", 1, &counter).await.unwrap();
        assert_eq!(b.shared.live.load(Ordering::SeqCst), VISIBILITY_DRAINERS);
        until("settled", || counter.load(Ordering::Relaxed) == 1).await;
        until("drainers idle out", || {
            b.shared.live.load(Ordering::SeqCst) == 0
        })
        .await;
        b.submit("two", 1, &counter).await.unwrap();
        until("restarted drainers settle it", || {
            counter.load(Ordering::Relaxed) == 2
        })
        .await;
    }

    #[tokio::test]
    async fn dropping_the_batcher_flushes_queued_changes() {
        let fake = Arc::new(FakeSender::gated());
        let counter = Arc::new(AtomicU64::new(0));
        {
            let b = batcher(&fake, VISIBILITY_CAPACITY, VISIBILITY_IDLE_EXIT);
            for i in 0..25 {
                b.submit(&format!("r{i}"), 1, &counter).await.unwrap();
            }
        }
        fake.open();
        until("flushed after drop", || {
            counter.load(Ordering::Relaxed) == 25
        })
        .await;
    }
}

#[cfg(test)]
mod pending_deletes_tests {
    use super::*;

    const GRACE: Duration = Duration::from_secs(5);
    const SECS: fn(u64) -> Duration = Duration::from_secs;

    #[test]
    fn in_flight_entry_survives_pruning_however_long() {
        let base = Instant::now();
        let mut p = PendingDeletes::default();
        p.insert("slow");
        p.prune(base + SECS(3600), GRACE);
        assert!(p.contains("slow"));
    }

    #[test]
    fn completed_entry_expires_grace_after_completion() {
        let base = Instant::now();
        let mut p = PendingDeletes::default();
        let gen = p.insert("a");
        // Acked long ago, completed only now: the grace runs from completion.
        let done = base + SECS(100);
        p.complete("a", gen, done);
        p.prune(done + SECS(4), GRACE);
        assert!(p.contains("a"));
        p.prune(done + SECS(5), GRACE);
        assert!(!p.contains("a"));
        assert!(p.order.is_empty());
    }

    #[test]
    fn failed_delete_is_forgotten_after_the_grace_too() {
        // The drainer completes an entry whatever its outcome; the map has no
        // notion of success, so a failure expires exactly like a success.
        let base = Instant::now();
        let mut p = PendingDeletes::default();
        let gen = p.insert("bad");
        p.complete("bad", gen, base);
        assert!(p.contains("bad"));
        p.prune(base + GRACE, GRACE);
        assert!(!p.contains("bad"));
    }

    #[test]
    fn reacked_id_survives_its_old_completion() {
        let base = Instant::now();
        let mut p = PendingDeletes::default();
        let g1 = p.insert("x");
        p.complete("x", g1, base);
        let g2 = p.insert("x"); // acked again, in flight
        p.prune(base + SECS(10), GRACE);
        assert!(p.contains("x"), "the newer in-flight ack must stay");
        // A late completion for the superseded generation is ignored.
        p.complete("x", g1, base + SECS(11));
        p.prune(base + SECS(100), GRACE);
        assert!(p.contains("x"));
        p.complete("x", g2, base + SECS(20));
        p.prune(base + SECS(25), GRACE);
        assert!(!p.contains("x"));
        assert!(p.order.is_empty());
    }

    #[test]
    fn in_flight_entries_do_not_block_later_completions() {
        let base = Instant::now();
        let mut p = PendingDeletes::default();
        let _slow = p.insert("slow");
        let g = p.insert("fast");
        p.complete("fast", g, base);
        p.prune(base + GRACE, GRACE);
        assert!(!p.contains("fast"));
        assert!(p.contains("slow"));
    }
}

#[cfg(test)]
mod receipt_map_tests {
    use super::*;

    const TTL: Duration = Duration::from_secs(10);

    fn filled(n: usize, at: Instant) -> ReceiptMap {
        let mut m = ReceiptMap::default();
        for i in 0..n {
            m.insert(&format!("h{i}"), format!("m{i}"), at);
        }
        m
    }

    #[test]
    fn expired_removed_fresh_kept() {
        let base = Instant::now();
        let mut m = filled(1001, base);
        m.insert("fresh", "mf".into(), base + Duration::from_secs(8));
        m.prune(base + Duration::from_secs(11), TTL);
        assert_eq!(m.map.len(), 1);
        assert!(m.map.contains_key("fresh"));
        assert_eq!(m.order.len(), 1);
    }

    #[test]
    fn reinserted_receipt_survives_its_old_fifo_entry() {
        let base = Instant::now();
        let mut m = filled(1001, base);
        m.insert("h0", "m0b".into(), base + Duration::from_secs(8));
        m.prune(base + Duration::from_secs(11), TTL);
        assert_eq!(m.map.get("h0").map(|(id, _)| id.as_str()), Some("m0b"));
        assert_eq!(m.map.len(), 1);
    }

    #[test]
    fn removed_entry_leaves_harmless_fifo_entry() {
        let base = Instant::now();
        let mut m = filled(1002, base);
        assert_eq!(m.remove("h5").as_deref(), Some("m5"));
        assert_eq!(m.remove("h5"), None);
        m.prune(base + Duration::from_secs(11), TTL);
        assert!(m.map.is_empty());
        assert!(m.order.is_empty());
    }

    #[test]
    fn small_map_still_prunes_expired() {
        let base = Instant::now();
        let mut m = filled(5, base);
        let examined = m.prune(base + Duration::from_secs(100), TTL);
        assert_eq!(examined, 5);
        assert!(m.map.is_empty());
        assert!(m.order.is_empty());
    }

    #[test]
    fn insert_then_ack_all_drains_fifo() {
        let base = Instant::now();
        let n = 10_000;
        let mut m = filled(n, base);
        for i in 0..n {
            assert!(m.remove(&format!("h{i}")).is_some());
        }
        // Nothing is expired; the stale entries must go regardless of age.
        let examined = m.prune(base + Duration::from_secs(1), TTL);
        assert_eq!(examined, n);
        assert!(m.map.is_empty());
        assert_eq!(m.order.len(), 0);
    }

    #[test]
    fn fifo_stays_bounded_behind_a_live_front_entry() {
        let base = Instant::now();
        let mut m = ReceiptMap::default();
        m.insert("stuck", "ms".into(), base);
        for i in 0..10_000 {
            let h = format!("h{i}");
            m.insert(&h, format!("m{i}"), base + Duration::from_secs(1));
            m.remove(&h);
            m.prune(base + Duration::from_secs(2), TTL);
            assert!(m.order.len() <= 2 * m.map.len() + ReceiptMap::COMPACT_SLACK + 1);
        }
        assert!(m.map.contains_key("stuck"));
    }

    #[test]
    fn reinserted_after_remove_survives_stale_entry() {
        let base = Instant::now();
        let mut m = ReceiptMap::default();
        m.insert("h", "m1".into(), base);
        m.remove("h");
        m.insert("h", "m2".into(), base + Duration::from_secs(1));
        m.prune(base + Duration::from_secs(2), TTL);
        assert_eq!(m.map.get("h").map(|(id, _)| id.as_str()), Some("m2"));
        assert_eq!(m.order.len(), 1);
    }

    #[test]
    fn prune_examined_counts_only_popped_entries() {
        let base = Instant::now();
        let mut m = filled(100, base + Duration::from_secs(5));
        // Live, unexpired: zero examined however large the map is.
        assert_eq!(m.prune(base + Duration::from_secs(6), TTL), 0);
        m.remove("h0");
        m.remove("h1");
        assert_eq!(m.prune(base + Duration::from_secs(6), TTL), 2);
        assert_eq!(m.map.len(), 98);
        assert_eq!(m.order.len(), 98);
    }

    #[test]
    fn prune_cost_tracks_expired_not_size() {
        let base = Instant::now();
        let mut m = filled(50_000, base + Duration::from_secs(5));
        // Nothing expired: no entry examined beyond the front peek.
        assert_eq!(m.prune(base + Duration::from_secs(6), TTL), 0);
        assert_eq!(m.map.len(), 50_000);
        // Three old entries ahead of the fresh ones.
        let mut old = ReceiptMap::default();
        for i in 0..3 {
            old.insert(&format!("o{i}"), format!("om{i}"), base);
        }
        old.order.append(&mut m.order);
        old.map.extend(m.map.drain());
        let examined = old.prune(base + Duration::from_secs(11), TTL);
        assert_eq!(examined, 3);
        assert_eq!(old.map.len(), 50_000);
    }
}
