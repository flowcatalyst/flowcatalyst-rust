//! NATS JetStream Queue Consumer
//!
//! Provides a pull-based JetStream consumer for NATS.
//! Supports:
//! - Pull-based message consumption with configurable batch sizes
//! - Manual acknowledgment with ack/nak/in-progress semantics
//! - Durable consumers with configurable ack wait and max deliver
//! - Automatic stream and consumer provisioning
//! - Queue metrics from JetStream consumer info

use async_nats::jetstream::{self, consumer::PullConsumer, stream, AckKind};
use async_trait::async_trait;
use dashmap::DashMap;
use futures::StreamExt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::{QueueConsumer, QueueError, QueueMetrics, RejectedLog, RejectedMessage, Result};
use fc_common::QueuedMessage;

/// Configuration for the NATS JetStream consumer
#[derive(Debug, Clone)]
pub struct NatsConfig {
    /// NATS server URL(s), comma-separated (e.g., "nats://localhost:4222")
    pub servers: String,
    /// JetStream stream name
    pub stream_name: String,
    /// Durable consumer name
    pub consumer_name: String,
    /// Subject filter for the consumer (e.g., "flowcatalyst.>")
    pub subject: String,
    /// Max messages to request per poll batch. Also sizes the client-side
    /// bounded channel the standing subscription feeds (item 2, owner
    /// ruling 2026-09-07) — "one batch client-side" of flow control.
    pub max_messages_per_poll: u32,
    /// Timeout in milliseconds for each poll/fetch request.
    ///
    /// **Unused since item 2** (owner ruling 2026-09-07, applied to Java
    /// and Go already): the no-wait/waiting `fetch()` pair this used to
    /// bound is gone, replaced by one continuous pull subscription per
    /// queue (`consumer.stream().messages()`) feeding a bounded channel;
    /// `poll()` now takes the first buffered message with an untimed,
    /// stop-cancellable await, so there is no per-poll timeout to apply.
    /// Still parsed from the URI (`from_uri` below) for wire compatibility
    /// with existing queue configs that set it — an operator's config
    /// naming a value that's now a no-op should not fail to parse.
    pub poll_timeout_ms: u64,
    /// Ack wait time in seconds before redelivery
    pub ack_wait_secs: u64,
    /// Maximum number of delivery attempts before the server gives up on a
    /// message. `-1` (the default) is unlimited — see [`NatsConfig::default`].
    pub max_deliver: i64,
    /// Maximum number of unacknowledged messages the consumer can have
    /// in-flight. `-1` (the default) is unlimited — see
    /// [`NatsConfig::default`].
    pub max_ack_pending: i64,
    /// Stream storage type: "file" or "memory"
    pub storage: String,
    /// Number of stream replicas (for clustering)
    pub replicas: usize,
    /// Maximum message age in days (0 = unlimited)
    pub max_age_days: u64,
}

impl NatsConfig {
    /// Parse a `nats://` queue URI into a [`NatsConfig`], per
    /// `docs/spec/router.md` §7.4:
    ///
    /// `nats://host:port[,host2…]?stream=&consumer=&subject=&max-messages=&poll-timeout-ms=&ack-wait-secs=&max-deliver=&max-ack-pending=&storage=file|memory&replicas=&max-age-days=`
    ///
    /// Everything before the first `?` (including the `nats://` scheme) is
    /// passed straight through as `servers` — `async_nats`'s connector
    /// already accepts a comma-separated multi-host string in that shape.
    /// Every query parameter is optional; an absent or unparseable one
    /// falls back to [`NatsConfig::default`]'s value for that field (same
    /// defaults the other routers ship: stream `FLOWCATALYST`, consumer
    /// `fc-router`, subject `flowcatalyst.>`, batch 10, poll timeout 20s,
    /// ack-wait 120s, max-deliver -1 (unlimited), max-ack-pending -1
    /// (unlimited), storage file, replicas 1, max-age 7d).
    pub fn from_uri(uri: &str) -> Result<Self> {
        if !uri.starts_with("nats://") {
            return Err(QueueError::Config(format!("not a nats:// URI: {}", uri)));
        }

        let mut config = NatsConfig::default();

        let (servers, query) = match uri.split_once('?') {
            Some((before, after)) => (before, Some(after)),
            None => (uri, None),
        };
        config.servers = servers.to_string();

        let Some(query) = query else {
            return Ok(config);
        };

        for pair in query.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (key, value) = match pair.split_once('=') {
                Some((k, v)) => (k, v),
                None => (pair, ""),
            };
            match key {
                "stream" if !value.is_empty() => config.stream_name = value.to_string(),
                "consumer" if !value.is_empty() => config.consumer_name = value.to_string(),
                "subject" if !value.is_empty() => config.subject = value.to_string(),
                "max-messages" => {
                    if let Ok(v) = value.parse::<u32>() {
                        config.max_messages_per_poll = v;
                    }
                }
                "poll-timeout-ms" => {
                    if let Ok(v) = value.parse::<u64>() {
                        config.poll_timeout_ms = v;
                    }
                }
                "ack-wait-secs" => {
                    if let Ok(v) = value.parse::<u64>() {
                        config.ack_wait_secs = v;
                    }
                }
                "max-deliver" => {
                    if let Ok(v) = value.parse::<i64>() {
                        config.max_deliver = v;
                    }
                }
                "max-ack-pending" => {
                    if let Ok(v) = value.parse::<i64>() {
                        config.max_ack_pending = v;
                    }
                }
                "storage" if !value.is_empty() => config.storage = value.to_string(),
                "replicas" => {
                    if let Ok(v) = value.parse::<usize>() {
                        config.replicas = v;
                    }
                }
                "max-age-days" => {
                    if let Ok(v) = value.parse::<u64>() {
                        config.max_age_days = v;
                    }
                }
                _ => {
                    // Unknown parameter — ignore rather than fail, same
                    // tolerance the other backends' URI parsing gives an
                    // operator adding a forward-looking query param.
                }
            }
        }

        Ok(config)
    }
}

/// Defaults match Go's `nats.DefaultConfig`.
///
/// `max_deliver` and `max_ack_pending` are unlimited (`-1`), as Go's are
/// (owner ruling 2026-09-22: "the router owns give-up"). The router releases
/// work back to the broker on its own schedule — every capacity deferral and
/// every nack spends one JetStream delivery — so a finite `max_deliver` turns
/// a slow pool's backlog into messages the server silently stops
/// redelivering. A message NAKed with a delay also stays ack-pending for the
/// whole delay, so a finite `max_ack_pending` lets one deferred backlog
/// suspend delivery for the entire stream. A cap can still be set per URI
/// (`max-deliver=`, `max-ack-pending=`) where poison-message protection is
/// wanted.
impl Default for NatsConfig {
    fn default() -> Self {
        Self {
            servers: "nats://localhost:4222".to_string(),
            stream_name: "FLOWCATALYST".to_string(),
            consumer_name: "fc-router".to_string(),
            subject: "flowcatalyst.>".to_string(),
            max_messages_per_poll: 10,
            poll_timeout_ms: 20_000, // Java default: 20 seconds
            ack_wait_secs: 120,      // Java default: 120 seconds
            max_deliver: -1,         // unlimited (Go; owner ruling 2026-09-22)
            max_ack_pending: -1,     // unlimited (Go; owner ruling 2026-09-22)
            storage: "file".to_string(),
            replicas: 1,
            max_age_days: 7,
        }
    }
}

/// NATS JetStream pull-based queue consumer.
///
/// Item 2 (owner ruling 2026-09-07, applied to Java and Go already): backed
/// by ONE continuous pull subscription per queue
/// (`consumer.stream().messages()`, `async-nats`'s server-driven
/// `Consumer<Config>` stream, not the old per-`poll()` `fetch()`/`batch()`
/// pair) rather than polling the broker on demand. A background task
/// (spawned in [`Self::new`], stopped via `stream_cancel`) drains that
/// subscription and pushes decoded [`QueuedMessage`]s into a bounded
/// channel of capacity `max_messages_per_poll` — "one batch client-side" of
/// flow control: the channel fills only as fast as `poll()` drains it, and
/// `async-nats`'s stream (via its own internal batch/heartbeat protocol)
/// only pulls more from the server as the channel has room, so a slow
/// consumer naturally throttles the pull rate instead of the client
/// buffering an unbounded backlog.
pub struct NatsQueueConsumer {
    config: NatsConfig,
    /// Cached queue identifier in Java format: `streamName/consumerName`
    queue_id: String,
    client: async_nats::Client,
    consumer: Arc<RwLock<PullConsumer>>,
    running: AtomicBool,
    /// Maps receipt handle (`streamName:streamSequence`) -> JetStream message for ack/nack
    pending_messages: Arc<DashMap<String, async_nats::jetstream::Message>>,
    /// Receiving end of the standing subscription's bounded channel. A
    /// `Mutex` rather than requiring `&mut self` on `poll()` — the trait
    /// takes `&self` (shared across `Arc<dyn QueueConsumer>`), and in
    /// production exactly one task ever calls `poll()` on a given consumer
    /// (`spawn_consumer_poll_task`'s single loop per consumer), so this
    /// lock is never contended in practice; it exists for soundness, not
    /// throughput.
    receiver: tokio::sync::Mutex<mpsc::Receiver<QueuedMessage>>,
    /// Cancels the background subscription-draining task — see
    /// [`Self::stop`]. Cancelling it also unblocks a `poll()` parked on
    /// `receiver.recv()`: the background task's `tx` (channel sender) is
    /// owned by that task and drops with it, which closes the channel.
    stream_cancel: CancellationToken,
    /// Total messages polled from queue
    total_polled: AtomicU64,
    /// Total messages successfully ACKed
    total_acked: AtomicU64,
    /// Total messages NACKed (actual failures)
    total_nacked: AtomicU64,
    /// Total messages deferred (rate limiting, capacity - not failures)
    total_deferred: AtomicU64,
    /// G13 (`docs/go-mirror/2026-09-06-go-fix-list.md`,
    /// `QueueConsumer::last_broker_activity`): last time the background
    /// subscription-draining task actually handed a message to `tx`.
    /// Read as the liveness fallback when the client doesn't report
    /// `Connected` (see `last_broker_activity` below); while connected,
    /// the connection state itself is the positive signal, since jnats-
    /// style clients expose no per-heartbeat callback, only a negative one
    /// for a *missed* heartbeat. Seeded to the consumer's construction
    /// time so a queue that has never delivered anything yet still reads
    /// as "recently alive" rather than a stale zero value.
    last_delivery: Arc<Mutex<Instant>>,
    /// False from the moment the standing subscription ends or fails
    /// terminally until a resubscribe succeeds (Go G14: `Queue.healthy`).
    /// While false, [`QueueConsumer::last_broker_activity`] stops vouching
    /// for the consumer, so the router's bounded poll reads a quiet
    /// `poll()` as an error rather than an idle queue, and — if
    /// resubscribing keeps failing — its stall watchdog rebuilds the whole
    /// consumer.
    subscription_healthy: Arc<AtomicBool>,
    /// Messages terminated because their payload could not be decoded.
    rejected: Arc<RejectedLog>,
}

/// Open the standing pull subscription with the options every
/// (re)subscribe uses, so the initial subscribe and a later resubscribe can
/// never drift apart (Go: the shared `resubscribe` closure).
/// `max_messages_per_batch` matches the channel capacity — the client asks
/// the server for at most one channel's worth of messages at a time;
/// `heartbeat` matches `.messages()`'s own convenience-method default (15s)
/// so an idle stream doesn't silently look dead.
async fn open_subscription(
    consumer: &PullConsumer,
    batch_size: usize,
) -> std::result::Result<
    async_nats::jetstream::consumer::pull::Stream,
    async_nats::jetstream::consumer::StreamError,
> {
    consumer
        .stream()
        .max_messages_per_batch(batch_size)
        .heartbeat(Duration::from_secs(15))
        .messages()
        .await
}

/// Re-open the standing subscription after it died. The consumer's
/// existence is confirmed on the server first: opening a pull subscription
/// never checks it, so without this a deleted consumer would "resubscribe"
/// successfully, read as healthy, and then wait for ever on pull requests
/// nothing answers.
async fn resubscribe(
    consumer: &PullConsumer,
    batch_size: usize,
) -> std::result::Result<async_nats::jetstream::consumer::pull::Stream, QueueError> {
    let mut probe = consumer.clone();
    probe
        .info()
        .await
        .map_err(|e| QueueError::nats("consumer info", e))?;
    open_subscription(consumer, batch_size)
        .await
        .map_err(|e| QueueError::nats("open subscription", e))
}

/// Everything the background forwarding task owns.
struct Forwarder {
    stream: async_nats::jetstream::consumer::pull::Stream,
    consumer: PullConsumer,
    batch_size: usize,
    stream_name: String,
    consumer_name: String,
    queue_id: String,
    pending_messages: Arc<DashMap<String, async_nats::jetstream::Message>>,
    cancel: CancellationToken,
    last_delivery: Arc<Mutex<Instant>>,
    healthy: Arc<AtomicBool>,
    rejected: Arc<RejectedLog>,
    tx: mpsc::Sender<QueuedMessage>,
}

/// Is `err` from the standing subscription one it cannot recover from on
/// its own? A consumer that was deleted, or a subscription the server
/// refuses, needs a fresh subscription (Go: any non-heartbeat `Next` error
/// triggers a resubscribe). A missed heartbeat or a failed pull request is
/// re-issued by async-nats internally, so the loop just keeps draining.
fn is_terminal_subscription_error(
    err: &async_nats::jetstream::consumer::pull::MessagesError,
) -> bool {
    use async_nats::jetstream::consumer::pull::MessagesErrorKind;
    matches!(
        err.kind(),
        MessagesErrorKind::ConsumerDeleted | MessagesErrorKind::PushBasedConsumer
    )
}

/// The one task that drains the standing subscription into the bounded
/// channel `poll()` reads.
///
/// When the subscription ends or fails terminally it is re-opened with
/// capped exponential backoff (200ms → 5s) until that succeeds or the
/// consumer is stopped — Go's `forward`/`resubscribeUntilHealthy`. It used
/// to simply exit, which closed the channel, made every later `poll()`
/// return `Stopped`, and left a consumer that would never deliver again.
async fn forward(mut f: Forwarder) {
    loop {
        let next = tokio::select! {
            _ = f.cancel.cancelled() => return,
            n = f.stream.next() => n,
        };

        let js_msg = match next {
            Some(Ok(msg)) => msg,
            Some(Err(e)) if !is_terminal_subscription_error(&e) => {
                // A transient protocol/connection error on one pull request
                // — async-nats re-issues pull requests internally; log and
                // keep draining.
                warn!(
                    consumer = %f.consumer_name,
                    error = %e,
                    "Error on NATS JetStream standing subscription"
                );
                continue;
            }
            ended => {
                match ended {
                    Some(Err(e)) => error!(
                        consumer = %f.consumer_name,
                        error = %e,
                        "NATS JetStream standing subscription failed; resubscribing"
                    ),
                    _ => warn!(
                        consumer = %f.consumer_name,
                        "NATS JetStream standing subscription ended; resubscribing"
                    ),
                }
                f.healthy.store(false, Ordering::SeqCst);
                let mut backoff = Duration::from_millis(200);
                loop {
                    if f.cancel.is_cancelled() {
                        return;
                    }
                    match resubscribe(&f.consumer, f.batch_size).await {
                        Ok(stream) => {
                            f.stream = stream;
                            f.healthy.store(true, Ordering::SeqCst);
                            info!(
                                consumer = %f.consumer_name,
                                "NATS resubscribed; subscription healthy again"
                            );
                            break;
                        }
                        Err(e) => {
                            error!(
                                consumer = %f.consumer_name,
                                error = %e,
                                backoff_ms = backoff.as_millis() as u64,
                                "NATS resubscribe failed; retrying"
                            );
                            tokio::select! {
                                _ = f.cancel.cancelled() => return,
                                _ = tokio::time::sleep(backoff) => {}
                            }
                            backoff = (backoff * 2).min(Duration::from_secs(5));
                        }
                    }
                }
                continue;
            }
        };

        let receipt_handle =
            match NatsQueueConsumer::receipt_handle_from_message(&js_msg, &f.stream_name) {
                Some(h) => h,
                None => {
                    warn!(
                        consumer = %f.consumer_name,
                        "Could not extract stream sequence from NATS message, skipping"
                    );
                    let _ = js_msg.ack_with(AckKind::Term).await;
                    continue;
                }
            };

        let message: fc_common::Message = match serde_json::from_slice(&js_msg.payload) {
            Ok(m) => m,
            Err(e) => {
                error!(
                    consumer = %f.consumer_name,
                    error = %e,
                    "Failed to parse NATS message payload, terminating message"
                );
                let seq = js_msg.info().ok().map(|i| i.stream_sequence.to_string());
                f.rejected.record(seq, e.to_string());
                let _ = js_msg.ack_with(AckKind::Term).await;
                continue;
            }
        };

        let broker_message_id = js_msg.info().ok().map(|info| {
            NatsQueueConsumer::broker_message_id_from_sequence(
                info.stream_sequence,
                info.consumer_sequence,
            )
        });

        // A redelivery replaces the pending entry: same stream sequence,
        // but only the newest delivery can be acked.
        f.pending_messages.insert(receipt_handle.clone(), js_msg);

        // G13: a real message just arrived from the broker — stamp the
        // liveness fallback the moment it did, not only once `poll()`
        // eventually hands it to a caller.
        if let Ok(mut guard) = f.last_delivery.lock() {
            *guard = Instant::now();
        }

        let queued = QueuedMessage {
            message,
            receipt_handle,
            broker_message_id,
            queue_identifier: f.queue_id.clone(),
        };

        // Race the send against cancellation too — a `stop()` that lands
        // while the channel is momentarily full (poll() not draining, e.g.
        // mid-shutdown) must not block this task's exit.
        tokio::select! {
            _ = f.cancel.cancelled() => return,
            res = f.tx.send(queued) => {
                if res.is_err() {
                    // Receiver dropped (consumer torn down) — nothing left
                    // to feed.
                    return;
                }
            }
        }
    }
}

impl NatsQueueConsumer {
    /// Create a new NATS JetStream consumer.
    ///
    /// This connects to the NATS server, ensures the stream exists (creates it if needed),
    /// and ensures the durable consumer exists (creates it if needed).
    pub async fn new(config: NatsConfig) -> Result<Self> {
        info!(
            servers = %config.servers,
            stream = %config.stream_name,
            consumer = %config.consumer_name,
            subject = %config.subject,
            "Connecting to NATS JetStream"
        );

        // Connect to NATS with reconnect settings matching Java:
        // unlimited reconnects, 2s reconnect wait, 10s connection timeout
        let client = async_nats::ConnectOptions::new()
            .connection_timeout(Duration::from_secs(10))
            .reconnect_delay_callback(|_attempts| Duration::from_secs(2))
            .connect(&config.servers)
            .await
            .map_err(|e| QueueError::nats("Failed to connect to NATS", e))?;

        info!(servers = %config.servers, "Connected to NATS");

        // Create JetStream context
        let jetstream = jetstream::new(client.clone());

        // Resolve storage type
        let storage_type = match config.storage.to_lowercase().as_str() {
            "memory" => stream::StorageType::Memory,
            _ => stream::StorageType::File,
        };

        // Calculate max age duration
        let max_age = if config.max_age_days > 0 {
            Duration::from_secs(config.max_age_days * 24 * 60 * 60)
        } else {
            Duration::ZERO // 0 means unlimited in NATS
        };

        // Ensure stream exists (create or get)
        // WorkQueue retention: messages are removed once consumed (Java default)
        let stream = jetstream
            .get_or_create_stream(stream::Config {
                name: config.stream_name.clone(),
                subjects: vec![config.subject.clone()],
                retention: stream::RetentionPolicy::WorkQueue,
                storage: storage_type,
                num_replicas: config.replicas,
                max_age,
                ..Default::default()
            })
            .await
            .map_err(|e| {
                QueueError::nats(
                    format!("Failed to get/create stream '{}'", config.stream_name),
                    e,
                )
            })?;

        info!(
            stream = %config.stream_name,
            storage = %config.storage,
            replicas = config.replicas,
            "JetStream stream ready"
        );

        // Create the durable consumer, or UPDATE it if it already exists
        // (Go: `CreateOrUpdateConsumer`). `get_or_create_consumer` returned
        // an existing consumer untouched, so a consumer provisioned under
        // the old finite defaults (max-deliver 10, max-ack-pending 1000)
        // kept them for ever; create-or-update applies the configured
        // limits to it on the next start.
        let consumer: PullConsumer = stream
            .create_consumer(jetstream::consumer::pull::Config {
                name: Some(config.consumer_name.clone()),
                durable_name: Some(config.consumer_name.clone()),
                ack_wait: Duration::from_secs(config.ack_wait_secs),
                max_deliver: config.max_deliver,
                max_ack_pending: config.max_ack_pending,
                filter_subject: config.subject.clone(),
                ..Default::default()
            })
            .await
            .map_err(|e| {
                QueueError::nats(
                    format!(
                        "Failed to create/update consumer '{}'",
                        config.consumer_name
                    ),
                    e,
                )
            })?;

        info!(
            consumer = %config.consumer_name,
            ack_wait_secs = config.ack_wait_secs,
            max_deliver = config.max_deliver,
            max_ack_pending = config.max_ack_pending,
            "JetStream consumer ready"
        );

        let queue_id = format!("{}/{}", config.stream_name, config.consumer_name);
        let pending_messages: Arc<DashMap<String, async_nats::jetstream::Message>> =
            Arc::new(DashMap::new());

        // Item 2: open the standing subscription and spawn the task that
        // drains it into `rx`'s bounded channel. `max_messages_per_batch`
        // matches the channel capacity — the client asks the server for at
        // most one channel's worth of messages at a time; `heartbeat`
        // matches `.messages()`'s own convenience-method default (15s) so
        // an idle stream doesn't silently look dead.
        let batch_size = (config.max_messages_per_poll.max(1)) as usize;
        let (tx, rx) = mpsc::channel::<QueuedMessage>(batch_size);
        let stream_cancel = CancellationToken::new();
        let last_delivery = Arc::new(Mutex::new(Instant::now()));

        let jetstream_stream = open_subscription(&consumer, batch_size)
            .await
            .map_err(|e| {
                QueueError::nats(
                    format!(
                        "Failed to open standing pull subscription for consumer '{}'",
                        config.consumer_name
                    ),
                    e,
                )
            })?;
        let subscription_healthy = Arc::new(AtomicBool::new(true));
        let rejected = Arc::new(RejectedLog::default());

        tokio::spawn(forward(Forwarder {
            stream: jetstream_stream,
            consumer: consumer.clone(),
            batch_size,
            stream_name: config.stream_name.clone(),
            consumer_name: config.consumer_name.clone(),
            queue_id: queue_id.clone(),
            pending_messages: pending_messages.clone(),
            cancel: stream_cancel.clone(),
            last_delivery: last_delivery.clone(),
            healthy: subscription_healthy.clone(),
            rejected: rejected.clone(),
            tx,
        }));

        Ok(Self {
            config,
            queue_id,
            client,
            consumer: Arc::new(RwLock::new(consumer)),
            running: AtomicBool::new(true),
            pending_messages,
            receiver: tokio::sync::Mutex::new(rx),
            stream_cancel,
            total_polled: AtomicU64::new(0),
            total_acked: AtomicU64::new(0),
            total_nacked: AtomicU64::new(0),
            total_deferred: AtomicU64::new(0),
            last_delivery,
            subscription_healthy,
            rejected,
        })
    }

    /// Extract receipt handle from a JetStream message.
    /// Format: `streamName:streamSequence`
    fn receipt_handle_from_message(
        msg: &async_nats::jetstream::Message,
        stream_name: &str,
    ) -> Option<String> {
        msg.info()
            .ok()
            .map(|info| format!("{}:{}", stream_name, info.stream_sequence))
    }

    /// Dedup id for a JetStream delivery (R-19).
    ///
    /// This MUST be the stream sequence only. The stream sequence is stable
    /// across redeliveries of the same message; the consumer sequence
    /// increments on every delivery attempt (including redeliveries), so
    /// folding it in gave every redelivery a fresh id and the dedup layer
    /// could never recognise a redelivery as the message it already saw —
    /// turning at-least-once delivery into at-most-once.
    fn broker_message_id_from_sequence(stream_sequence: u64, _consumer_sequence: u64) -> String {
        stream_sequence.to_string()
    }
}

#[async_trait]
impl QueueConsumer for NatsQueueConsumer {
    fn identifier(&self) -> &str {
        &self.queue_id
    }

    /// Item 2: takes the first message off the standing subscription's
    /// bounded channel with an **untimed**, cancellation-aware await (a
    /// `stop()` closes the channel — see `stream_cancel`'s doc comment —
    /// so a parked `recv()` resolves to `None` promptly rather than
    /// blocking forever), then drains whatever else is *immediately*
    /// available (`try_recv`, non-blocking) up to `max_messages`. This
    /// replaces the old no-wait-then-waiting `fetch()` pair one-for-one at
    /// the `poll()` call boundary: still "return promptly if something's
    /// there, otherwise wait for the next thing," just backed by a
    /// continuously-fed local buffer instead of two separate broker round
    /// trips per call. `poll_timeout_ms` is no longer consulted — see its
    /// field doc.
    async fn poll(&self, max_messages: u32) -> Result<Vec<QueuedMessage>> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(QueueError::Stopped);
        }

        let limit = max_messages.min(self.config.max_messages_per_poll).max(1) as usize;
        let mut rx = self.receiver.lock().await;

        let first = match rx.recv().await {
            Some(m) => m,
            // Channel closed: either `stop()` cancelled the background
            // task (which then dropped its `tx`), or the subscription
            // itself ended. Either way this consumer is done.
            None => return Err(QueueError::Stopped),
        };

        let mut messages = Vec::with_capacity(limit);
        messages.push(first);
        while messages.len() < limit {
            match rx.try_recv() {
                Ok(m) => messages.push(m),
                Err(_) => break,
            }
        }
        drop(rx);

        self.total_polled
            .fetch_add(messages.len() as u64, Ordering::Relaxed);
        debug!(
            consumer = %self.config.consumer_name,
            count = messages.len(),
            "Polled messages from NATS JetStream (standing subscription)"
        );

        Ok(messages)
    }

    async fn ack(&self, receipt_handle: &str) -> Result<()> {
        let (_, js_msg) = self
            .pending_messages
            .remove(receipt_handle)
            .ok_or_else(|| {
                QueueError::NotFound(format!(
                    "No pending message for receipt handle: {}",
                    receipt_handle
                ))
            })?;

        js_msg
            .ack()
            .await
            .map_err(|e| QueueError::nats("Failed to ACK message", e))?;

        self.total_acked.fetch_add(1, Ordering::Relaxed);
        debug!(
            receipt_handle = %receipt_handle,
            consumer = %self.config.consumer_name,
            "Message acknowledged in NATS JetStream"
        );

        Ok(())
    }

    async fn nack(&self, receipt_handle: &str, delay_seconds: Option<u32>) -> Result<()> {
        let (_, js_msg) = self
            .pending_messages
            .remove(receipt_handle)
            .ok_or_else(|| {
                QueueError::NotFound(format!(
                    "No pending message for receipt handle: {}",
                    receipt_handle
                ))
            })?;

        let ack_kind = match delay_seconds {
            Some(secs) if secs > 0 => AckKind::Nak(Some(Duration::from_secs(secs as u64))),
            _ => AckKind::Nak(None),
        };

        js_msg
            .ack_with(ack_kind)
            .await
            .map_err(|e| QueueError::nats("Failed to NAK message", e))?;

        self.total_nacked.fetch_add(1, Ordering::Relaxed);
        debug!(
            receipt_handle = %receipt_handle,
            consumer = %self.config.consumer_name,
            delay_seconds = ?delay_seconds,
            "Message NACKed in NATS JetStream"
        );

        Ok(())
    }

    async fn defer(&self, receipt_handle: &str, delay_seconds: Option<u32>) -> Result<()> {
        let (_, js_msg) = self
            .pending_messages
            .remove(receipt_handle)
            .ok_or_else(|| {
                QueueError::NotFound(format!(
                    "No pending message for receipt handle: {}",
                    receipt_handle
                ))
            })?;

        let ack_kind = match delay_seconds {
            Some(secs) if secs > 0 => AckKind::Nak(Some(Duration::from_secs(secs as u64))),
            _ => AckKind::Nak(None),
        };

        js_msg
            .ack_with(ack_kind)
            .await
            .map_err(|e| QueueError::nats("Failed to defer message", e))?;

        self.total_deferred.fetch_add(1, Ordering::Relaxed);
        debug!(
            receipt_handle = %receipt_handle,
            consumer = %self.config.consumer_name,
            delay_seconds = ?delay_seconds,
            "Message deferred in NATS JetStream (not counted as failure)"
        );

        Ok(())
    }

    async fn extend_visibility(&self, receipt_handle: &str, _seconds: u32) -> Result<()> {
        let js_msg = self.pending_messages.get(receipt_handle).ok_or_else(|| {
            QueueError::NotFound(format!(
                "No pending message for receipt handle: {}",
                receipt_handle
            ))
        })?;

        // AckKind::Progress resets the ack_wait timer, giving the consumer more time
        // to process the message without it being redelivered.
        js_msg
            .value()
            .ack_with(AckKind::Progress)
            .await
            .map_err(|e| QueueError::nats("Failed to extend visibility (in-progress)", e))?;

        debug!(
            receipt_handle = %receipt_handle,
            consumer = %self.config.consumer_name,
            "Visibility extended (ack_wait reset) in NATS JetStream"
        );

        Ok(())
    }

    /// Go: `HonoursDelayedReturn() == false` for NATS — the stream is one
    /// durable WorkQueue consumer with no per-group subject, so a
    /// `NakWithDelay` never holds a delayed head's successors back.
    fn honours_delayed_return(&self) -> bool {
        false
    }

    fn take_rejected(&self) -> Vec<RejectedMessage> {
        self.rejected.take()
    }

    fn is_healthy(&self) -> bool {
        if !self.running.load(Ordering::SeqCst) {
            return false;
        }

        // Check the underlying NATS connection state
        matches!(
            self.client.connection_state(),
            async_nats::connection::State::Connected
        )
    }

    /// G13: "now" for as long as the connection reads `Connected` and the
    /// standing subscription is healthy (G14) — this
    /// client exposes no positive per-heartbeat callback (only a negative
    /// `ErrorListener`-style alarm for a *missed* one), so the connection
    /// state itself, refreshed on every check, stands in as the positive
    /// signal; falling back to the last time a message actually arrived
    /// otherwise (disconnected/reconnecting). Either way this can only
    /// read as "alive" for as long as one of those two things is
    /// genuinely true right now — a connection that drops and stays down
    /// ages `last_delivery` normally and eventually reads stale, so the
    /// stall watchdog still restarts it (G13's "never hides a real hang"
    /// requirement).
    fn last_broker_activity(&self) -> Option<Instant> {
        let subscribed =
            self.running.load(Ordering::SeqCst) && self.subscription_healthy.load(Ordering::SeqCst);
        if subscribed
            && matches!(
                self.client.connection_state(),
                async_nats::connection::State::Connected
            )
        {
            Some(Instant::now())
        } else {
            self.last_delivery.lock().ok().map(|g| *g)
        }
    }

    /// Stop INTAKE: cancelling `stream_cancel` stops the background
    /// subscription-draining task, which drops its channel `Sender` as it
    /// exits — that closes the channel, so any `poll()` currently parked on
    /// `receiver.recv()` (an untimed await) resolves to `None` and returns
    /// `Err(QueueError::Stopped)` promptly rather than staying blocked.
    ///
    /// Messages already handed out stay in `pending_messages`, and the
    /// connection stays open until the consumer itself is dropped, so the
    /// ack/nack of work still in flight when the consumer was stopped
    /// still reaches the server. This used to clear `pending_messages`,
    /// which failed every such ack: the work was then redelivered after
    /// `ack_wait` (120s), by which time the router's duplicate guard had
    /// long expired — a completed delivery sent a second time.
    async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        self.stream_cancel.cancel();

        let pending_count = self.pending_messages.len();
        info!(
            consumer = %self.config.consumer_name,
            pending_count,
            "NATS JetStream consumer stopped (in-flight messages can still be acked)"
        );
    }

    async fn get_metrics(&self) -> Result<Option<QueueMetrics>> {
        let mut consumer = self.consumer.write().await;

        let info = consumer
            .info()
            .await
            .map_err(|e| QueueError::nats("Failed to get consumer info", e))?;

        let pending_messages = info.num_pending;
        let in_flight_messages = info.num_ack_pending as u64;

        debug!(
            consumer = %self.config.consumer_name,
            pending = pending_messages,
            in_flight = in_flight_messages,
            redelivered = info.num_redelivered,
            "Retrieved NATS JetStream consumer metrics"
        );

        Ok(Some(QueueMetrics {
            pending_messages,
            in_flight_messages,
            queue_identifier: self.queue_id.clone(),
            total_polled: self.total_polled.load(Ordering::Relaxed),
            total_acked: self.total_acked.load(Ordering::Relaxed),
            total_nacked: self.total_nacked.load(Ordering::Relaxed),
            total_deferred: self.total_deferred.load(Ordering::Relaxed),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = NatsConfig::default();
        assert_eq!(config.servers, "nats://localhost:4222");
        assert_eq!(config.stream_name, "FLOWCATALYST");
        assert_eq!(config.consumer_name, "fc-router");
        assert_eq!(config.subject, "flowcatalyst.>");
        assert_eq!(config.max_messages_per_poll, 10);
        assert_eq!(config.poll_timeout_ms, 20_000); // Java: 20 seconds
        assert_eq!(config.ack_wait_secs, 120); // Java: 120 seconds
                                               // Go: unlimited (owner ruling 2026-09-22 — the router owns give-up).
        assert_eq!(config.max_deliver, -1);
        assert_eq!(config.max_ack_pending, -1);
        assert_eq!(config.storage, "file");
        assert_eq!(config.replicas, 1);
        assert_eq!(config.max_age_days, 7);
    }

    #[test]
    fn test_storage_type_parsing() {
        // Test that our storage parsing logic works
        let file_storage = match "file".to_lowercase().as_str() {
            "memory" => stream::StorageType::Memory,
            _ => stream::StorageType::File,
        };
        assert!(matches!(file_storage, stream::StorageType::File));

        let memory_storage = match "memory".to_lowercase().as_str() {
            "memory" => stream::StorageType::Memory,
            _ => stream::StorageType::File,
        };
        assert!(matches!(memory_storage, stream::StorageType::Memory));

        let memory_upper = match "MEMORY".to_lowercase().as_str() {
            "memory" => stream::StorageType::Memory,
            _ => stream::StorageType::File,
        };
        assert!(matches!(memory_upper, stream::StorageType::Memory));
    }

    /// R-19: broker_message_id must be the stream sequence only, so that two
    /// deliveries of the same message — which necessarily share a stream
    /// sequence but carry different (incrementing) consumer sequences — are
    /// recognised by the dedup layer as the same message.
    #[test]
    fn test_broker_message_id_ignores_consumer_sequence() {
        let first_delivery = NatsQueueConsumer::broker_message_id_from_sequence(42, 1);
        let redelivery = NatsQueueConsumer::broker_message_id_from_sequence(42, 7);

        assert_eq!(first_delivery, redelivery);
        assert_eq!(first_delivery, "42");
    }

    #[test]
    fn test_broker_message_id_distinguishes_different_messages() {
        let a = NatsQueueConsumer::broker_message_id_from_sequence(42, 1);
        let b = NatsQueueConsumer::broker_message_id_from_sequence(43, 1);
        assert_ne!(a, b);
    }

    // --- NatsConfig::from_uri (item 1: scheme-dispatched queue wiring) ---

    #[test]
    fn from_uri_bare_servers_uses_every_default() {
        let config = NatsConfig::from_uri("nats://localhost:4222").unwrap();
        let defaults = NatsConfig::default();
        assert_eq!(config.servers, "nats://localhost:4222");
        assert_eq!(config.stream_name, defaults.stream_name);
        assert_eq!(config.consumer_name, defaults.consumer_name);
        assert_eq!(config.subject, defaults.subject);
        assert_eq!(config.max_messages_per_poll, defaults.max_messages_per_poll);
        assert_eq!(config.poll_timeout_ms, defaults.poll_timeout_ms);
        assert_eq!(config.ack_wait_secs, defaults.ack_wait_secs);
        assert_eq!(config.max_deliver, defaults.max_deliver);
        assert_eq!(config.max_ack_pending, defaults.max_ack_pending);
        assert_eq!(config.storage, defaults.storage);
        assert_eq!(config.replicas, defaults.replicas);
        assert_eq!(config.max_age_days, defaults.max_age_days);
    }

    #[test]
    fn from_uri_parses_every_query_parameter() {
        let uri = "nats://host1:4222,host2:4222?stream=BENCH1&consumer=router&subject=flowcatalyst.bench.>&max-messages=25&poll-timeout-ms=5000&ack-wait-secs=60&max-deliver=5&max-ack-pending=500&storage=memory&replicas=3&max-age-days=1";
        let config = NatsConfig::from_uri(uri).unwrap();

        assert_eq!(config.servers, "nats://host1:4222,host2:4222");
        assert_eq!(config.stream_name, "BENCH1");
        assert_eq!(config.consumer_name, "router");
        assert_eq!(config.subject, "flowcatalyst.bench.>");
        assert_eq!(config.max_messages_per_poll, 25);
        assert_eq!(config.poll_timeout_ms, 5000);
        assert_eq!(config.ack_wait_secs, 60);
        assert_eq!(config.max_deliver, 5);
        assert_eq!(config.max_ack_pending, 500);
        assert_eq!(config.storage, "memory");
        assert_eq!(config.replicas, 3);
        assert_eq!(config.max_age_days, 1);
    }

    #[test]
    fn from_uri_identifier_is_stream_slash_consumer() {
        let config =
            NatsConfig::from_uri("nats://localhost:4222?stream=BENCH4&consumer=router").unwrap();
        assert_eq!(
            format!("{}/{}", config.stream_name, config.consumer_name),
            "BENCH4/router"
        );
    }

    #[test]
    fn from_uri_rejects_non_nats_scheme() {
        assert!(NatsConfig::from_uri("postgres://host/db").is_err());
    }

    #[test]
    fn from_uri_ignores_unparseable_numeric_param_keeping_default() {
        // A malformed value must not poison the whole parse — it falls back
        // to the default for that one field, same tolerance the other
        // fields get when a param is absent altogether.
        let config =
            NatsConfig::from_uri("nats://localhost:4222?max-messages=not-a-number").unwrap();
        assert_eq!(
            config.max_messages_per_poll,
            NatsConfig::default().max_messages_per_poll
        );
    }
}
