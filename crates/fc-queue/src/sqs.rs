use async_trait::async_trait;
use aws_sdk_sqs::types::{
    ChangeMessageVisibilityBatchRequestEntry, DeleteMessageBatchRequestEntry,
};
use aws_sdk_sqs::{types::Message as SqsMessage, types::QueueAttributeName, Client};
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use tokio::sync::{mpsc, oneshot, Mutex as AsyncMutex};
use tokio::time::timeout;
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

/// Long-lived drainer tasks per queue, so one slow round trip does not cap the
/// queue at `DELETE_BATCH_MAX` deletes per round trip.
const DELETE_DRAINERS: usize = 4;

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

struct DeleteItem {
    receipt_handle: String,
    done: oneshot::Sender<EntryOutcome>,
}

/// Coalesces concurrent acks into `DeleteMessageBatch` calls. No fill window:
/// a drainer sends whatever is already waiting (up to `DELETE_BATCH_MAX`).
struct DeleteBatcher {
    tx: mpsc::UnboundedSender<DeleteItem>,
    closed: Arc<AtomicBool>,
}

impl DeleteBatcher {
    /// Must be called inside a tokio runtime (spawns the drainers).
    fn start(sender: Arc<dyn DeleteBatchSender>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel::<DeleteItem>();
        let rx = Arc::new(AsyncMutex::new(rx));
        let closed = Arc::new(AtomicBool::new(false));
        for _ in 0..DELETE_DRAINERS {
            tokio::spawn(Self::drain(rx.clone(), sender.clone(), closed.clone()));
        }
        Self { tx, closed }
    }

    async fn drain(
        rx: Arc<AsyncMutex<mpsc::UnboundedReceiver<DeleteItem>>>,
        sender: Arc<dyn DeleteBatchSender>,
        closed: Arc<AtomicBool>,
    ) {
        loop {
            let mut batch: Vec<DeleteItem> = Vec::with_capacity(DELETE_BATCH_MAX);
            {
                let mut rx = rx.lock().await;
                match rx.recv().await {
                    Some(item) => batch.push(item),
                    None => return,
                }
                while batch.len() < DELETE_BATCH_MAX {
                    match rx.try_recv() {
                        Ok(item) => batch.push(item),
                        Err(_) => break,
                    }
                }
            }
            if closed.load(Ordering::SeqCst) {
                // Shut down: dropping the items fails their waiters.
                return;
            }
            let receipts: Vec<String> = batch.iter().map(|i| i.receipt_handle.clone()).collect();
            match sender.send_batch(&receipts).await {
                Ok(outcomes) => {
                    for (i, item) in batch.into_iter().enumerate() {
                        let outcome = outcomes
                            .get(i)
                            .cloned()
                            .unwrap_or_else(|| Err("no result for batch entry".to_string()));
                        let _ = item.done.send(outcome);
                    }
                }
                Err(e) => {
                    for item in batch {
                        let _ = item.done.send(Err(e.clone()));
                    }
                }
            }
        }
    }

    /// Delete one receipt, resolving once the broker answered for it.
    async fn delete(&self, receipt_handle: &str) -> EntryOutcome {
        if self.closed.load(Ordering::SeqCst) {
            return Err("delete batcher is shut down".to_string());
        }
        let (done, wait) = oneshot::channel();
        let item = DeleteItem {
            receipt_handle: receipt_handle.to_string(),
            done,
        };
        if self.tx.send(item).is_err() {
            return Err("delete batcher is shut down".to_string());
        }
        match wait.await {
            Ok(outcome) => outcome,
            Err(_) => Err("delete batcher shut down before the delete completed".to_string()),
        }
    }

    /// Stop accepting work; items still queued fail instead of hanging.
    fn shutdown(&self) {
        self.closed.store(true, Ordering::SeqCst);
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

/// Acked-message-id guard with an insertion-ordered FIFO so expiry pops from
/// the front only (one clock read per prune) instead of scanning the map.
#[derive(Default)]
struct PendingDeletes {
    map: HashMap<String, Instant>,
    order: VecDeque<(String, Instant)>,
}

impl PendingDeletes {
    fn insert(&mut self, id: String, at: Instant) {
        self.map.insert(id.clone(), at);
        self.order.push_back((id, at));
    }

    /// Drop entries whose age at `now` is >= `ttl`. A FIFO entry superseded by
    /// a later re-insert of the same id is discarded without touching the map.
    fn prune(&mut self, now: Instant, ttl: Duration) {
        while let Some((_, ts)) = self.order.front() {
            if now.saturating_duration_since(*ts) < ttl {
                break;
            }
            let Some((id, ts)) = self.order.pop_front() else {
                break;
            };
            if self.map.get(&id) == Some(&ts) {
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
#[derive(Default)]
struct ReceiptMap {
    map: HashMap<String, (String, Instant)>,
    order: VecDeque<(String, Instant)>,
}

impl ReceiptMap {
    /// Prune only once the map holds more than this many entries.
    const PRUNE_THRESHOLD: usize = 1000;

    fn insert(&mut self, handle: String, msg_id: String, at: Instant) {
        self.map.insert(handle.clone(), (msg_id, at));
        self.order.push_back((handle, at));
    }

    fn remove(&mut self, handle: &str) -> Option<String> {
        self.map.remove(handle).map(|(id, _)| id)
    }

    /// If the map is over the threshold, drop entries whose age at `now`
    /// is at least `ttl`. A FIFO entry superseded by a later re-insert of the same
    /// handle (or already removed) is discarded without touching the map.
    ///
    /// Returns how many FIFO entries were examined (popped).
    fn prune(&mut self, now: Instant, ttl: Duration) -> usize {
        if self.map.len() <= Self::PRUNE_THRESHOLD {
            return 0;
        }
        let mut examined = 0;
        while let Some((_, ts)) = self.order.front() {
            if now.saturating_duration_since(*ts) < ttl {
                break;
            }
            let Some((handle, ts)) = self.order.pop_front() else {
                break;
            };
            examined += 1;
            if self.map.get(&handle).is_some_and(|(_, t)| *t == ts) {
                self.map.remove(&handle);
            }
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
    /// redelivery within the TTL is batch-deleted without re-routing to the
    /// mediator. Entries age out after `PENDING_DELETE_TTL`.
    pending_delete_ids: Mutex<PendingDeletes>,
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

    /// How long to remember an acked SQS MessageId so redeliveries are
    /// short-circuited to DeleteMessage without re-routing to the mediator.
    const PENDING_DELETE_TTL: Duration = Duration::from_secs(15 * 60);

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
            pending_delete_ids: Mutex::new(PendingDeletes::default()),
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
            // delete the redelivery immediately and move on. Keep the entry until
            // it ages out past the TTL so every redelivery within the window is
            // short-circuited — not just the first one.
            if let Some(msg_id) = sqs_msg.message_id() {
                let should_delete = {
                    let mut pending = self.pending_delete_ids.lock();
                    pending.prune(Instant::now(), Self::PENDING_DELETE_TTL);
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
                        map.prune(now, Self::PENDING_DELETE_TTL);
                        map.insert(receipt_handle.clone(), msg_id.clone(), now);
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
        // Always record the MessageId in pending_delete_ids — regardless of
        // whether DeleteMessage succeeds or fails. SQS standard queues are
        // at-least-once, so even a successful delete can be followed by a
        // redelivery; a failed delete obviously needs the same guard.
        // Redeliveries within the TTL are batch-deleted in `poll` without
        // being re-routed to the mediator.
        let msg_id = self.receipt_to_message_id.lock().remove(receipt_handle);
        if let Some(ref id) = msg_id {
            self.pending_delete_ids
                .lock()
                .insert(id.clone(), Instant::now());
        }

        let batcher = self.delete_batcher.get_or_init(|| {
            DeleteBatcher::start(Arc::new(SqsDeleteSender {
                client: self.client.clone(),
                queue_url: self.queue_url.clone(),
            }))
        });

        match batcher.delete(receipt_handle).await {
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
    }

    impl FakeSender {
        fn new() -> Self {
            Self {
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
            if !self.delay.is_zero() {
                sleep(self.delay).await;
            }
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
        let batcher = Arc::new(DeleteBatcher::start(fake.clone()));
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
    async fn lone_ack_succeeds_promptly() {
        let fake = Arc::new(FakeSender::new());
        let batcher = DeleteBatcher::start(fake.clone());
        let out = timeout(Duration::from_millis(500), batcher.delete("only"))
            .await
            .expect("a lone ack must not wait for a fill window");
        assert_eq!(out, Ok(()));
        assert_eq!(*fake.sizes.lock(), vec![1]);
    }

    #[tokio::test]
    async fn per_entry_failure_fails_only_that_ack() {
        let fake = Arc::new(FakeSender {
            fail_receipts: vec!["bad".to_string()],
            delay: Duration::from_millis(30),
            ..FakeSender::new()
        });
        let batcher = Arc::new(DeleteBatcher::start(fake));
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
        let batcher = Arc::new(DeleteBatcher::start(fake));
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
        let batcher = Arc::new(DeleteBatcher::start(fake));
        let mut tasks = Vec::new();
        for i in 0..(DELETE_DRAINERS * DELETE_BATCH_MAX + 20) {
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

    const TTL: Duration = Duration::from_secs(10);

    #[test]
    fn expired_removed_fresh_kept() {
        let base = Instant::now();
        let mut p = PendingDeletes::default();
        p.insert("old".into(), base);
        p.insert("fresh".into(), base + Duration::from_secs(8));
        p.prune(base + Duration::from_secs(11), TTL);
        assert!(!p.contains("old"));
        assert!(p.contains("fresh"));
        assert_eq!(p.order.len(), 1);
    }

    #[test]
    fn reinserted_id_survives_its_old_fifo_entry() {
        let base = Instant::now();
        let mut p = PendingDeletes::default();
        p.insert("x".into(), base);
        p.insert("x".into(), base + Duration::from_secs(8));
        // The first entry expires; the id was re-inserted so it must stay.
        p.prune(base + Duration::from_secs(11), TTL);
        assert!(p.contains("x"));
        // Later the newer entry expires too.
        p.prune(base + Duration::from_secs(19), TTL);
        assert!(!p.contains("x"));
        assert!(p.order.is_empty());
    }
}

#[cfg(test)]
mod receipt_map_tests {
    use super::*;

    const TTL: Duration = Duration::from_secs(10);

    fn filled(n: usize, at: Instant) -> ReceiptMap {
        let mut m = ReceiptMap::default();
        for i in 0..n {
            m.insert(format!("h{i}"), format!("m{i}"), at);
        }
        m
    }

    #[test]
    fn expired_removed_fresh_kept() {
        let base = Instant::now();
        let mut m = filled(1001, base);
        m.insert("fresh".into(), "mf".into(), base + Duration::from_secs(8));
        m.prune(base + Duration::from_secs(11), TTL);
        assert_eq!(m.map.len(), 1);
        assert!(m.map.contains_key("fresh"));
        assert_eq!(m.order.len(), 1);
    }

    #[test]
    fn reinserted_receipt_survives_its_old_fifo_entry() {
        let base = Instant::now();
        let mut m = filled(1001, base);
        m.insert("h0".into(), "m0b".into(), base + Duration::from_secs(8));
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
    fn no_prune_at_or_under_threshold() {
        let base = Instant::now();
        let mut m = filled(ReceiptMap::PRUNE_THRESHOLD, base);
        let examined = m.prune(base + Duration::from_secs(100), TTL);
        assert_eq!(examined, 0);
        assert_eq!(m.map.len(), ReceiptMap::PRUNE_THRESHOLD);
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
            old.insert(format!("o{i}"), format!("om{i}"), base);
        }
        old.order.append(&mut m.order);
        old.map.extend(m.map.drain());
        let examined = old.prune(base + Duration::from_secs(11), TTL);
        assert_eq!(examined, 3);
        assert_eq!(old.map.len(), 50_000);
    }
}
