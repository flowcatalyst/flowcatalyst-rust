//! Batch routing: `QueueManager::route_batch` and the private helpers it
//! calls — duplicate filtering, the R-13/R-16 strict-routing gate, and the
//! pool/message-group grouping passes. Also home to
//! [`QueueMessageCallback`], the [`fc_common::MessageCallback`] impl
//! `route_batch` hands to each pool worker.

use futures::future;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, error, info, warn};

use dashmap::DashMap;
use fc_common::{
    BatchMessage, InFlightMessage, MessageCallback, QueuedMessage, WarningCategory, WarningSeverity,
};
use fc_queue::QueueConsumer;

use crate::error::RouterError;
use crate::Result;

use super::tracking::Tracked;
use super::{ConsumerRegistry, QueueManager, RunningConsumer};

/// Scope a broker-native message id to the queue it came from (G11,
/// `docs/go-mirror/2026-09-06-go-fix-list.md`).
///
/// A broker id is only unique *within one queue* — NATS JetStream's is
/// `<streamSeq>:<consumerSeq>`-shaped per stream, so a second NATS queue's
/// Nth message collides with the first queue's Nth message on the bare id.
/// Every map keyed on a broker id (`in_pipeline`'s `pipeline_key`,
/// `pending_delete_broker_ids`) MUST use this scoped form instead of the
/// bare broker id, or two queues sharing a sequence number cross-wire their
/// tracker entries — an ack on one queue's receipt handle winds up applied
/// (and rejected by the broker) on a different queue's consumer. `\0` is the
/// separator: it cannot appear in either half (a queue identifier or a
/// broker id), unlike `:` or `/`, both of which NATS identifiers already
/// contain.
fn broker_scope_key(queue_identifier: &str, broker_id: &str) -> String {
    format!("{queue_identifier}\0{broker_id}")
}

/// Bound on one ack/nack broker call made by a callback. A broker call that
/// never returns would otherwise pin the pool worker — and, for an ordered
/// group, every message behind it — indefinitely. An ack that times out is
/// treated like a failed ack (the broker id goes to pending-delete, so its
/// redelivery is deleted on sight); a nack that times out leaves the message
/// to the broker's own visibility timeout. Go sets no such bound.
const BROKER_OP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Callback that the pool worker calls directly when processing completes.
/// Reads the latest receipt handle from in_pipeline (may have been swapped by
/// redelivery), performs the broker operation, then cleans up tracking.
/// No spawned task, no channel — mirrors the TS closure pattern.
///
/// **Ownership (H12).** The callback acts on the tracker entry only while
/// the entry still carries this admission's `generation`. If the entry was
/// reaped and a later copy of the same message admitted in its place, this
/// callback neither clears that copy's entry nor uses its receipt handle:
/// an ack goes out on this copy's own route-time handle, and a nack is
/// skipped (releasing the message would pull it out from under the copy
/// that now owns it).
///
/// **Drop safety.** If this callback is dropped without `ack()` or `nack()`
/// being called (panic during mediation, runtime cancellation, abandoned
/// queue task on early drain-task exit, …) the `Drop` impl clears this
/// admission's tracking synchronously and fires a best-effort `nack`, so
/// broker redeliveries are not swallowed as duplicates of a dead owner.
struct QueueMessageCallback {
    pipeline_key: String,
    app_message_id: String,
    /// The queue this message was polled from — `Consumer::identifier()`.
    /// Used (G11) to scope the `pending_delete` broker-id key the same way
    /// `pipeline_key` itself is scoped, so an ACK-failed retry never
    /// collides with another queue's entry at the same broker id.
    queue_identifier: String,
    /// This admission's tracker generation — see the type's doc.
    generation: u64,
    /// The entry as it was admitted, with this copy's own receipt handle:
    /// what an ack falls back to when the entry is gone, and what
    /// `ensure_tracked` restores.
    admitted: InFlightMessage,
    /// The consumer instance that received the message — the last-resort
    /// target when nothing in the registry answers for its queue (a message
    /// routed by a consumer the manager never registered).
    origin: Arc<dyn QueueConsumer>,
    /// Generation of the registered instance that received the message, or
    /// 0 when it was not registered.
    origin_generation: u64,
    /// The manager's consumer registry: ack/nack resolve their consumer
    /// through it when they run (Go: `resolveConsumer`), not through an
    /// instance captured at route time that may since have been replaced.
    registry: Arc<ConsumerRegistry>,
    in_pipeline: Arc<DashMap<String, Tracked>>,
    app_message_to_pipeline_key: Arc<DashMap<String, String>>,
    pending_delete: Arc<DashMap<String, Instant>>,
    /// Set to true the moment `ack()` or `nack()` is entered. The `Drop`
    /// impl checks this and only fires fallback cleanup if no resolution
    /// happened. AcqRel ordering: the load in Drop must observe stores from
    /// any thread that called ack/nack.
    completed: std::sync::atomic::AtomicBool,
}

/// Whose tracker entry sits under this callback's pipeline key.
enum Ownership {
    /// This admission's: act with the entry's freshest receipt handle.
    Owned {
        receipt_handle: String,
        broker_message_id: Option<String>,
    },
    /// No entry (reaped, force-acked, or shutdown cleared it).
    Gone,
    /// A later admission of the same message owns it.
    Other,
}

impl QueueMessageCallback {
    /// The consumer to ack/nack through, resolved now — see
    /// [`ConsumerRegistry::resolve`].
    fn consumer(&self) -> Arc<dyn QueueConsumer> {
        self.registry
            .resolve(&self.queue_identifier, self.origin_generation)
            .unwrap_or_else(|| self.origin.clone())
    }

    fn ownership(&self) -> Ownership {
        match self.in_pipeline.get(&self.pipeline_key) {
            Some(e) if e.generation == self.generation => Ownership::Owned {
                receipt_handle: e.receipt_handle.clone(),
                broker_message_id: e.broker_message_id.clone(),
            },
            Some(_) => Ownership::Other,
            None => Ownership::Gone,
        }
    }

    /// Drop this admission's tracking entries — never a later copy's — so
    /// future redeliveries flow through again instead of being swallowed
    /// as duplicates.
    fn cleanup_tracking(&self) {
        let removed = self
            .in_pipeline
            .remove_if(&self.pipeline_key, |_, e| e.generation == self.generation)
            .is_some();
        if removed {
            self.app_message_to_pipeline_key
                .remove_if(&self.app_message_id, |_, k| *k == self.pipeline_key);
        }
    }
}

#[async_trait::async_trait]
impl MessageCallback for QueueMessageCallback {
    /// Go: `EnsureTracked` — restore this admission's entry if it was
    /// reaped while the message sat buffered; `false` when a different
    /// broker copy of the same application message now owns the pipeline.
    fn ensure_tracked(&self) -> bool {
        if self.in_pipeline.contains_key(&self.pipeline_key) {
            return true;
        }
        if let Some(key) = self
            .app_message_to_pipeline_key
            .get(&self.app_message_id)
            .map(|k| k.value().clone())
        {
            if key != self.pipeline_key && self.in_pipeline.contains_key(&key) {
                return false;
            }
        }
        self.in_pipeline
            .entry(self.pipeline_key.clone())
            .or_insert_with(|| Tracked::new(self.admitted.clone(), self.generation));
        self.app_message_to_pipeline_key
            .insert(self.app_message_id.clone(), self.pipeline_key.clone());
        true
    }

    /// Go: `MarkRetrying`.
    fn mark_retrying(&self) {
        if let Some(mut e) = self.in_pipeline.get_mut(&self.pipeline_key) {
            if e.generation == self.generation {
                e.mark_retrying();
            }
        }
    }

    /// Go: the source consumer's `HonoursDelayedReturn` (NATS: false).
    fn honours_delayed_return(&self) -> bool {
        self.consumer().honours_delayed_return()
    }

    async fn ack(&self) {
        // Mark resolved BEFORE doing any await so the Drop impl knows we
        // owned the resolution even if a panic happens mid-await.
        self.completed
            .store(true, std::sync::atomic::Ordering::Release);

        let (handle, broker_id) = match self.ownership() {
            Ownership::Owned {
                receipt_handle,
                broker_message_id,
            } => (receipt_handle, broker_message_id),
            Ownership::Gone => (
                self.admitted.receipt_handle.clone(),
                self.admitted.broker_message_id.clone(),
            ),
            Ownership::Other => {
                warn!(
                    app_message_id = %self.app_message_id,
                    "ACK for a copy whose tracker entry now belongs to a later admission; acking with this copy's own receipt"
                );
                (
                    self.admitted.receipt_handle.clone(),
                    self.admitted.broker_message_id.clone(),
                )
            }
        };

        let acked =
            match tokio::time::timeout(BROKER_OP_TIMEOUT, self.consumer().ack(&handle)).await {
                Ok(r) => r.map_err(|e| e.to_string()),
                Err(_) => Err(format!("ack did not complete within {BROKER_OP_TIMEOUT:?}")),
            };
        if let Err(e) = acked {
            if let Some(ref bid) = broker_id {
                warn!(
                    broker_message_id = %bid,
                    app_message_id = %self.app_message_id,
                    error = %e,
                    "ACK failed (receipt handle likely expired) - adding to pending delete"
                );
                // G11: scope by queue, same as `pipeline_key` — see
                // `broker_scope_key`'s doc comment.
                self.pending_delete.insert(
                    broker_scope_key(&self.queue_identifier, bid),
                    Instant::now(),
                );
            } else {
                error!(
                    app_message_id = %self.app_message_id,
                    error = %e,
                    "ACK failed and no broker message ID to track for pending delete"
                );
            }
        }

        // Clean up tracking AFTER the broker operation
        self.cleanup_tracking();
    }

    async fn nack(&self, delay_seconds: Option<u32>) {
        // Mark resolved BEFORE doing any await; see ack() above.
        self.completed
            .store(true, std::sync::atomic::Ordering::Release);

        let handle = match self.ownership() {
            Ownership::Owned { receipt_handle, .. } => Some(receipt_handle),
            Ownership::Gone => Some(self.admitted.receipt_handle.clone()),
            Ownership::Other => {
                warn!(
                    app_message_id = %self.app_message_id,
                    "NACK skipped — a later admission of this message owns it"
                );
                None
            }
        };
        if let Some(handle) = handle {
            match tokio::time::timeout(
                BROKER_OP_TIMEOUT,
                self.consumer().nack(&handle, delay_seconds),
            )
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    debug!(app_message_id = %self.app_message_id, error = %e, "NACK failed; the broker redelivers at its own timeout")
                }
                Err(_) => warn!(
                    app_message_id = %self.app_message_id,
                    "NACK did not complete within {:?}; the broker redelivers at its own timeout",
                    BROKER_OP_TIMEOUT
                ),
            }
        }

        // Clean up tracking AFTER the broker operation
        self.cleanup_tracking();
    }
}

impl Drop for QueueMessageCallback {
    fn drop(&mut self) {
        // Fast path: ack() or nack() ran, no fallback needed.
        if self.completed.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }

        // The callback was dropped without resolution (mediator panic, task
        // cancellation, an abandoned drain task). Clear this admission's
        // tracking so redeliveries are not swallowed, and fire a
        // best-effort nack so the message returns sooner than its natural
        // visibility timeout.
        let handle = match self.ownership() {
            Ownership::Owned { receipt_handle, .. } => Some(receipt_handle),
            Ownership::Gone => Some(self.admitted.receipt_handle.clone()),
            Ownership::Other => None,
        };

        // Synchronous cleanup of tracking — never deferred.
        self.cleanup_tracking();

        warn!(
            pipeline_key = %self.pipeline_key,
            app_message_id = %self.app_message_id,
            "Callback dropped without ack/nack — fallback cleanup ran (likely mediator panic or task cancel)"
        );

        if let Some(handle) = handle {
            if let Ok(rt) = tokio::runtime::Handle::try_current() {
                let consumer = self.consumer();
                rt.spawn(async move {
                    let _ = consumer.nack(&handle, Some(10)).await;
                });
            }
        }
    }
}

/// Reports why `msg` is malformed under strict routing
/// (`FC_ROUTER_STRICT_ROUTING`; see [`QueueManagerBuilder::strict_routing`](super::QueueManagerBuilder::strict_routing)),
/// or `None` if well-formed. Checked once per message at route time, before
/// pool resolution — every condition here is exactly what non-strict routing
/// papers over with a fallback (`DEFAULT-POOL`, the A-09 dispatch-mode
/// default, or a shared ordered group), so under strict routing none of them
/// may be silently repaired.
fn malformed_routing_reason(msg: &fc_common::Message) -> Option<&'static str> {
    if msg.pool_code.is_empty() {
        return Some("empty pool_code");
    }
    if !msg.dispatch_mode_specified {
        return Some("empty dispatch_mode");
    }
    if msg.dispatch_mode.requires_ordering()
        && msg.message_group_id.as_deref().unwrap_or("").is_empty()
    {
        return Some("ordered dispatch_mode with no message_group_id");
    }
    None
}

impl QueueManager {
    /// Route a batch of messages polled by `consumer`. Callbacks resolve the
    /// consumer through the registry when they ack/nack, falling back to
    /// `consumer` itself when it isn't registered.
    pub async fn route_batch(
        &self,
        messages: Vec<QueuedMessage>,
        consumer: Arc<dyn QueueConsumer>,
    ) -> Result<()> {
        self.route_batch_inner(messages, consumer, 0)
            .await
            .map(|_| ())
    }

    /// Route a batch polled by the registered consumer `rc`, recording on it
    /// the pools the batch fed and the messages deferred for capacity — the
    /// inputs to its own capacity gate (Go: `setPools`, the deferral
    /// ledger).
    pub(super) async fn route_batch_from(
        &self,
        messages: Vec<QueuedMessage>,
        rc: &RunningConsumer,
    ) -> Result<()> {
        let outcome = self
            .route_batch_inner(messages, rc.consumer.clone(), rc.generation)
            .await?;
        rc.set_dest_pools(outcome.fed_pools);
        let due =
            Instant::now() + std::time::Duration::from_secs(Self::CAPACITY_DEFER_SECONDS as u64);
        for _ in 0..outcome.deferred {
            rc.note_deferral(due);
        }
        Ok(())
    }

    /// Delay on a message handed back because its pool was full.
    const CAPACITY_DEFER_SECONDS: u32 = 5;

    async fn route_batch_inner(
        &self,
        messages: Vec<QueuedMessage>,
        consumer: Arc<dyn QueueConsumer>,
        origin_generation: u64,
    ) -> Result<RouteOutcome> {
        let mut outcome = RouteOutcome::default();
        if !self.running.load(Ordering::SeqCst) {
            // NACK all messages concurrently on shutdown
            let nack_futs: Vec<_> = messages
                .iter()
                .map(|msg| {
                    let consumer = consumer.clone();
                    let handle = msg.receipt_handle.clone();
                    async move {
                        let _ = consumer.nack(&handle, None).await;
                    }
                })
                .collect();
            future::join_all(nack_futs).await;
            return Err(RouterError::ShutdownInProgress);
        }

        if messages.is_empty() {
            return Ok(outcome);
        }

        let batch_id: Arc<str> = Arc::from(
            self.batch_counter
                .fetch_add(1, Ordering::Relaxed)
                .to_string()
                .as_str(),
        );

        // Phase 0: Check for messages that need immediate deletion (previously processed but ACK failed)
        // First, identify which messages need deletion. `pending_delete_broker_ids`
        // is a DashMap (concurrency-audit consolidation #3) — each check is
        // an independent per-key removal, not one lock held across the
        // whole batch scan.
        let mut messages_to_delete = Vec::new();
        let mut messages_to_process = Vec::with_capacity(messages.len());
        for msg in messages {
            // G11: scoped by queue — see `broker_scope_key`'s doc comment.
            let should_delete = msg
                .broker_message_id
                .as_ref()
                .map(|broker_id| {
                    self.pending_delete_broker_ids
                        .remove(&broker_scope_key(&msg.queue_identifier, broker_id))
                        .is_some()
                })
                .unwrap_or(false);

            if should_delete {
                // This message was already processed successfully, mark for deletion
                messages_to_delete.push(msg);
            } else {
                messages_to_process.push(msg);
            }
        }
        // Perform the deletions concurrently (independent SQS API calls)
        if !messages_to_delete.is_empty() {
            let delete_futs: Vec<_> = messages_to_delete
                .iter()
                .map(|msg| {
                    let consumer = consumer.clone();
                    let handle = msg.receipt_handle.clone();
                    let broker_id = msg.broker_message_id.clone();
                    let app_id = msg.message.id.clone();
                    async move {
                        info!(
                            broker_message_id = ?broker_id,
                            app_message_id = %app_id,
                            "Message was previously processed - deleting from queue now"
                        );
                        let _ = consumer.ack(&handle).await;
                    }
                })
                .collect();
            future::join_all(delete_futs).await;
        }

        if messages_to_process.is_empty() {
            return Ok(outcome);
        }

        // Phase 1: Filter duplicates (takes ownership to avoid cloning payloads)
        let filtered = self.filter_duplicates(messages_to_process);

        // Handle duplicates - no SQS API call needed.
        // filter_duplicates() already updated the receipt handle in in_pipeline,
        // so the eventual ACK will use the latest valid handle from this redelivery.
        // We intentionally do NOT defer/nack here — the message stays in SQS with its
        // natural visibility timeout. When it expires SQS redelivers, we update the
        // handle again, and this repeats until processing completes and we ACK with
        // the latest handle. This matches the Java behavior and avoids a hot
        // poll-defer loop that inflates SQS metrics and wastes API calls.
        if !filtered.duplicates.is_empty() {
            debug!(
                count = filtered.duplicates.len(),
                "Duplicate messages (redelivery) — receipt handles updated, no SQS action needed"
            );
        }

        // Handle requeued - these were already completed, ACK them
        // ACK requeued duplicates concurrently
        if !filtered.requeued.is_empty() {
            let requeue_futs: Vec<_> = filtered.requeued.iter().map(|req| {
                let consumer = consumer.clone();
                let handle = req.message.receipt_handle.clone();
                let msg_id = req.message.message.id.clone();
                let key = req.existing_pipeline_key.clone();
                async move {
                    debug!(message_id = %msg_id, pipeline_key = %key, "Requeued duplicate, ACKing");
                    let _ = consumer.ack(&handle).await;
                }
            }).collect();
            future::join_all(requeue_futs).await;
        }

        // Phase 1.5: R-13/R-16 strict routing gate. Only active under
        // FC_ROUTER_STRICT_ROUTING (off by default). A malformed message
        // (empty pool_code, unspecified dispatch_mode, or an ordered mode
        // with no message_group_id) is never fixable by the usual fallback
        // (DEFAULT-POOL, the A-09 default, a shared group) — under strict
        // routing that's a producer bug, not something to paper over. ACK
        // only: it must never be delivered, and never NACKed either, since
        // nothing about a retry would fix a malformed message. `unique`
        // messages here were never registered in `in_pipeline` (that
        // happens later, per-group, just before `pool.submit`), so there is
        // no tracker entry to release.
        let well_formed = if self.strict_routing {
            let mut well_formed = Vec::with_capacity(filtered.unique.len());
            let mut malformed_futs = Vec::new();
            for msg in filtered.unique {
                if let Some(reason) = malformed_routing_reason(&msg.message) {
                    warn!(
                        message_id = %msg.message.id,
                        queue = %consumer.identifier(),
                        reason = reason,
                        "Strict routing: malformed message; ACKing without delivery"
                    );
                    self.warning_service.add_warning(
                        WarningCategory::Configuration,
                        WarningSeverity::Warn,
                        format!(
                            "Malformed message {} on queue {}: {}",
                            msg.message.id,
                            consumer.identifier(),
                            reason
                        ),
                        "QueueManager".to_string(),
                    );
                    let consumer = consumer.clone();
                    let handle = msg.receipt_handle.clone();
                    let app_id = msg.message.id.clone();
                    let broker_id = msg.broker_message_id.clone();
                    malformed_futs.push(async move {
                        if let Err(e) = consumer.ack(&handle).await {
                            warn!(
                                message_id = %app_id,
                                broker_message_id = ?broker_id,
                                error = %e,
                                "ack (strict routing malformed) failed"
                            );
                        }
                    });
                } else {
                    well_formed.push(msg);
                }
            }
            future::join_all(malformed_futs).await;
            well_formed
        } else {
            filtered.unique
        };

        // Phase 2: Group by pool and route
        let by_pool = self.group_by_pool(well_formed).await;
        let fed_pools = &mut outcome.fed_pools;

        for (pool_code, pool_messages) in by_pool {
            let pool = match self.get_or_create_pool(&pool_code, None).await {
                Ok(p) => p,
                Err(e) => {
                    error!(pool_code = %pool_code, error = %e, "Failed to get/create pool");
                    // NACK all messages for this pool
                    for msg in pool_messages {
                        let _ = consumer.nack(&msg.receipt_handle, Some(5)).await;
                    }
                    continue;
                }
            };

            fed_pools.push(pool_code.clone());

            // Capacity is admitted message by message (Go: each message is
            // submitted and a full pool defers just that one). The whole
            // batch for a pool used to be deferred whenever it did not fit
            // entirely, so a pool with room for 9 of 10 took none, and the
            // consumer re-polled the same batch in a hot loop that inflated
            // SQS receive counts toward the DLQ. Within an ordered group,
            // once one message is deferred its successors are too.
            let fits_whole_batch = pool.available_capacity() >= pool_messages.len();
            if fits_whole_batch && pool.note_capacity_recovered() {
                info!(pool_code = %pool_code, "Pool capacity returned; resuming normal routing");
            }

            // Note: Rate limiting is now handled inside the pool worker (blocking wait)
            // Messages stay in pool queue instead of being deferred back to SQS

            // Phase 3: Group by messageGroupId for FIFO ordering enforcement
            // This mirrors Java's messagesByGroup logic in routeMessageBatch
            let messages_by_group = self.group_by_message_group(pool_messages);

            for (group_id, group_messages) in messages_by_group {
                let mut nack_remaining = false;
                let mut defer_remaining = false;

                for msg in group_messages {
                    if defer_remaining || pool.available_capacity() == 0 {
                        defer_remaining = true;
                        if pool.note_capacity_full() {
                            warn!(
                                pool_code = %pool_code,
                                "Pool at capacity, deferring what does not fit"
                            );
                            self.warning_service.add_warning(
                                WarningCategory::QueueHealth,
                                WarningSeverity::Warn,
                                format!(
                                    "Pool [{}] queue full, deferring messages that do not fit",
                                    pool_code
                                ),
                                "QueueManager".to_string(),
                            );
                        }
                        let _ = consumer
                            .defer(&msg.receipt_handle, Some(Self::CAPACITY_DEFER_SECONDS))
                            .await;
                        outcome.deferred += 1;
                        continue;
                    }

                    // If previous message in group failed, NACK all remaining in this group
                    // This enforces FIFO ordering - if message A fails, message B (which depends on A) must also fail
                    if nack_remaining {
                        debug!(
                            message_id = %msg.message.id,
                            group_id = %group_id,
                            "NACKing message - previous message in group failed submission"
                        );
                        let _ = consumer.nack(&msg.receipt_handle, Some(5)).await;
                        continue;
                    }

                    let app_message_id = msg.message.id.clone();

                    // Use broker_message_id, scoped to its queue (G11), as
                    // pipeline key (mirrors Java's sqsMessageId usage, plus
                    // the queue scoping Java's InFlightTracker applies to
                    // its own broker-id index) — a bare broker id is only
                    // unique within one queue (NATS: unique per stream), so
                    // two queues at the same sequence number would
                    // otherwise collide. Fall back to a composite key
                    // (already queue-scoped) if broker_message_id is not
                    // available.
                    let pipeline_key = msg
                        .broker_message_id
                        .as_deref()
                        .map(|bid| broker_scope_key(&msg.queue_identifier, bid))
                        .unwrap_or_else(|| {
                            format!("fallback:{}:{}", msg.queue_identifier, msg.message.id)
                        });

                    let receipt_handle = msg.receipt_handle.clone();

                    // Track in pipeline with receipt handle
                    let queue_identifier = msg.queue_identifier.clone();
                    let in_flight = InFlightMessage::new(
                        &msg.message,
                        msg.broker_message_id.clone(),
                        msg.queue_identifier.clone(),
                        Some(Arc::clone(&batch_id)),
                        msg.receipt_handle.clone(),
                    );
                    let generation =
                        self.next_tracker_generation.fetch_add(1, Ordering::Relaxed) + 1;
                    self.in_pipeline.insert(
                        pipeline_key.clone(),
                        Tracked::new(in_flight.clone(), generation),
                    );

                    // Track app message ID -> pipeline key for requeue detection
                    self.app_message_to_pipeline_key
                        .insert(app_message_id.clone(), pipeline_key.clone());

                    // Create callback — pool worker calls this directly, no spawned task
                    let callback = QueueMessageCallback {
                        pipeline_key: pipeline_key.clone(),
                        app_message_id: app_message_id.clone(),
                        queue_identifier,
                        generation,
                        admitted: in_flight,
                        origin: consumer.clone(),
                        origin_generation,
                        registry: self.consumers.clone(),
                        in_pipeline: self.in_pipeline.clone(),
                        app_message_to_pipeline_key: self.app_message_to_pipeline_key.clone(),
                        pending_delete: self.pending_delete_broker_ids.clone(),
                        completed: std::sync::atomic::AtomicBool::new(false),
                    };

                    let batch_msg = BatchMessage {
                        message: msg.message,
                        receipt_handle: msg.receipt_handle,
                        broker_message_id: msg.broker_message_id,
                        queue_identifier: msg.queue_identifier,
                        batch_id: Some(Arc::clone(&batch_id)),
                        callback: Box::new(callback),
                    };

                    // Submit to pool — pool worker calls callback.ack()/nack() when done
                    if let Err(e) = pool.submit(batch_msg).await {
                        error!(
                            message_id = %app_message_id,
                            group_id = %group_id,
                            error = %e,
                            "Failed to submit to pool - NACKing this and remaining messages in group"
                        );

                        // Remove from pipeline since we're NACKing
                        self.in_pipeline.remove(&pipeline_key);
                        self.app_message_to_pipeline_key.remove(&app_message_id);

                        // NACK this message
                        let _ = consumer.nack(&receipt_handle, Some(5)).await;

                        // Set flag to NACK all remaining messages in this group (FIFO enforcement)
                        nack_remaining = true;
                    }
                }
            }
        }

        Ok(outcome)
    }

    /// Filter duplicates from a batch.
    ///
    /// Mirrors Java's deduplication logic:
    /// 1. Check broker_message_id first (same SQS message = redelivery due to visibility timeout)
    /// 2. Check app_message_id second (same app ID, different broker ID = external requeue)
    ///
    /// Takes ownership of the messages Vec to avoid cloning payloads.
    fn filter_duplicates(&self, messages: Vec<QueuedMessage>) -> FilteredBatch {
        let mut result = FilteredBatch {
            unique: Vec::with_capacity(messages.len()),
            duplicates: Vec::new(),
            requeued: Vec::new(),
        };

        for msg in messages {
            // Check 1: Same broker message ID (physical redelivery from SQS due to visibility timeout)
            // This MUST be checked FIRST because the same broker ID means it's a visibility timeout redelivery,
            // NOT a requeue by an external process
            if let Some(ref broker_msg_id) = msg.broker_message_id {
                // G11: look up the queue-scoped key, not the bare broker id
                // — see `broker_scope_key`'s doc comment.
                let pipeline_key = broker_scope_key(&msg.queue_identifier, broker_msg_id);
                if let Some(mut entry) = self.in_pipeline.get_mut(&pipeline_key) {
                    // Adopt the redelivery's receipt handle so the eventual
                    // ACK uses the latest valid one, and refresh the idle
                    // clock the reaper judges the entry by (Go:
                    // UpdateReceiptHandle refreshes LastSeenAt).
                    debug!(
                        message_id = %msg.message.id,
                        broker_message_id = %broker_msg_id,
                        "Redelivery of an in-flight message (visibility timeout)"
                    );
                    entry.redelivered(&msg.receipt_handle);
                    // Also update broker_message_id in case it was a fallback key
                    if entry.broker_message_id.is_none() {
                        entry.broker_message_id = Some(broker_msg_id.clone());
                    }
                    drop(entry);
                    result.duplicates.push(DuplicateMessage {
                        message: msg,
                        existing_pipeline_key: pipeline_key,
                    });
                    continue;
                }
            }

            // Check 2: Same application message ID but DIFFERENT broker message ID (requeued by external process)
            // This happens when a separate process requeues messages that were stuck in QUEUED status for 20+ min
            // The external process creates a NEW SQS message with the same application message ID
            if let Some(existing_pipeline_key) =
                self.app_message_to_pipeline_key.get(&msg.message.id)
            {
                let existing_key = existing_pipeline_key.value().clone();

                // Only treat as requeued duplicate if the broker message IDs are DIFFERENT
                // If they're the same, it would have been caught by the check above.
                // `existing_key` is a queue-scoped pipeline_key (G11) — build
                // the same scoped form from this message's own broker id
                // before comparing, or the comparison always reads as
                // "different" now that the scoping prefix never matches a
                // bare id.
                if let Some(ref new_broker_id) = msg.broker_message_id {
                    let candidate_key = broker_scope_key(&msg.queue_identifier, new_broker_id);
                    if candidate_key != existing_key {
                        info!(
                            app_message_id = %msg.message.id,
                            existing_broker_id = %existing_key,
                            new_broker_id = %new_broker_id,
                            "Requeued message detected - app ID already in pipeline, will ACK to remove duplicate"
                        );
                        result.requeued.push(DuplicateMessage {
                            message: msg,
                            existing_pipeline_key: existing_key,
                        });
                        continue;
                    }
                }

                // Same broker ID or no broker ID - check if still in pipeline
                if let Some(mut entry) = self.in_pipeline.get_mut(&existing_key) {
                    // Redelivery: adopt the handle and refresh the idle clock.
                    entry.redelivered(&msg.receipt_handle);
                    result.duplicates.push(DuplicateMessage {
                        message: msg,
                        existing_pipeline_key: existing_key,
                    });
                    continue;
                }
            }

            result.unique.push(msg);
        }

        result
    }

    /// Group messages by pool code.
    /// Mirrors Java's pool routing logic: if a pool code is not found in processPools,
    /// log a ROUTING warning and fall back to DEFAULT-POOL.
    ///
    /// R-13: an empty `pool_code` warns exactly like an unknown one (this
    /// used to fall silently to DEFAULT-POOL with no warning at all — the
    /// same "papered over" anti-pattern strict routing exists to reject; a
    /// missing pool code is exactly as much a producer bug as a misspelled
    /// one).
    ///
    /// R-59: a code shaped like a per-client fallback
    /// (`{identifier}-DEFAULT-POOL`, and not exactly the global
    /// `DEFAULT-POOL`) is a *third* case, distinct from both the hit path
    /// and the unknown-code warning path above: if it doesn't exist yet it
    /// is synthesised on demand with default settings
    /// (`ensure_fallback_pool`) and routed to — no ROUTING warning, unlike
    /// a genuinely unknown code, because this is the expected shape for a
    /// short-lived client's traffic, not a producer bug. An already-tracked
    /// synthesised pool has its idle clock reset on every hit, not just at
    /// creation (`touch_synth_pool`), so eviction judges recency of traffic,
    /// not age since synthesis.
    async fn group_by_pool(
        &self,
        messages: Vec<QueuedMessage>,
    ) -> std::collections::HashMap<String, Vec<QueuedMessage>> {
        let mut by_pool: std::collections::HashMap<String, Vec<QueuedMessage>> =
            std::collections::HashMap::new();

        for msg in messages {
            let code = &msg.message.pool_code;
            let pool_code = if !code.is_empty() && self.active_pool(code).is_some() {
                // Existing pool — configured, or a fallback pool this
                // manager previously synthesised. Only the latter is
                // tracked in `synth_pools`, so this is a no-op for every
                // other pool code.
                if self.is_synth_pool_code(code) {
                    self.touch_synth_pool(code);
                }
                code.clone()
            } else if !code.is_empty() && self.is_synth_pool_code(code) {
                // R-59: per-client fallback pool, not seen before —
                // synthesise it rather than falling through to the
                // unknown-code warning below.
                match self.ensure_fallback_pool(code).await {
                    Ok(_) => code.clone(),
                    Err(e) => {
                        error!(
                            pool_code = %code,
                            message_id = %msg.message.id,
                            error = %e,
                            "Failed to synthesise per-client fallback pool; falling back to DEFAULT-POOL"
                        );
                        self.default_pool_code.clone()
                    }
                }
            } else {
                // Empty or unknown (non-fallback-shaped) pool_code → log
                // warning + route to DEFAULT-POOL.
                warn!(
                    message_id = %msg.message.id,
                    pool_code = %code,
                    default_pool = %self.default_pool_code,
                    "No pool found for pool_code, routing to DEFAULT-POOL"
                );
                self.warning_service.add_warning(
                    WarningCategory::Routing,
                    WarningSeverity::Warn,
                    format!(
                        "No pool found for code [{}] on message [{}] — routed to {}",
                        code, msg.message.id, self.default_pool_code
                    ),
                    "QueueManager".to_string(),
                );
                self.default_pool_code.clone()
            };

            by_pool.entry(pool_code).or_default().push(msg);
        }

        by_pool
    }

    /// Group messages by message_group_id for FIFO ordering enforcement
    /// (the per-batch NACK-cascade below: if one message in a group fails
    /// `pool.submit`, the rest of the group is NACKed for FIFO). Mirrors
    /// Java's messagesByGroup logic in routeMessageBatch.
    ///
    /// R-13: messages with no real ordered group (IMMEDIATE mode, or an
    /// ordered mode with no `message_group_id`) each get their own unique
    /// pseudo-group keyed by message id, rather than sharing one
    /// `"__DEFAULT__"` bucket. The shared bucket was the exact anti-pattern
    /// R-13 exists to delete: unrelated IMMEDIATE/groupless messages that
    /// happened to land in the same poll batch would NACK-cascade off each
    /// other on a submit failure, even though nothing actually links them.
    /// With strict routing off, this is the "ordered mode with no group id
    /// routes down the IMMEDIATE path" behaviour (Go parity, deliberate).
    fn group_by_message_group(
        &self,
        messages: Vec<QueuedMessage>,
    ) -> indexmap::IndexMap<String, Vec<QueuedMessage>> {
        // Use IndexMap to preserve insertion order (like Java's LinkedHashMap)
        let mut by_group: indexmap::IndexMap<String, Vec<QueuedMessage>> =
            indexmap::IndexMap::new();

        for msg in messages {
            let group_id = match &msg.message.message_group_id {
                Some(g) if !g.is_empty() && msg.message.dispatch_mode.requires_ordering() => {
                    g.clone()
                }
                _ => format!("__ungrouped__:{}", msg.message.id),
            };
            by_group.entry(group_id).or_default().push(msg);
        }

        by_group
    }
}

/// What routing one batch did, for the consumer's capacity gate.
#[derive(Default)]
struct RouteOutcome {
    /// Pools the batch was routed to.
    fed_pools: Vec<String>,
    /// Messages handed back to the broker because their pool was full.
    deferred: usize,
}

/// Result of filtering duplicates from a message batch
struct FilteredBatch {
    /// Messages that are new and should be processed
    unique: Vec<QueuedMessage>,
    /// Messages already in pipeline (redelivery due to visibility timeout) - NACK these
    duplicates: Vec<DuplicateMessage>,
    /// Messages requeued externally while original still processing - ACK these
    requeued: Vec<DuplicateMessage>,
}

/// A duplicate message with its existing pipeline key
struct DuplicateMessage {
    message: QueuedMessage,
    /// The pipeline key of the original message being processed
    existing_pipeline_key: String,
}

#[cfg(test)]
mod callback_drop_tests {
    use super::*;
    use crate::mediator::HttpMediatorConfig;
    use async_trait::async_trait;
    use fc_common::{Message, QueuedMessage};
    use fc_queue::Result as QueueResult;
    use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

    /// Records ack/nack calls for assertions in unit tests.
    #[derive(Default)]
    struct RecordingConsumer {
        acks: AtomicU32,
        nacks: AtomicU32,
    }

    #[async_trait]
    impl QueueConsumer for RecordingConsumer {
        fn identifier(&self) -> &str {
            "recording"
        }
        async fn poll(&self, _: u32) -> QueueResult<Vec<QueuedMessage>> {
            Ok(vec![])
        }
        async fn ack(&self, _: &str) -> QueueResult<()> {
            self.acks.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(())
        }
        async fn nack(&self, _: &str, _: Option<u32>) -> QueueResult<()> {
            self.nacks.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(())
        }
        async fn extend_visibility(&self, _: &str, _: u32) -> QueueResult<()> {
            Ok(())
        }
        fn is_healthy(&self) -> bool {
            true
        }
        async fn stop(&self) {}
    }

    // Test helper — tuple return is intentionally ad-hoc; a type alias
    // would only obscure intent for a single call site.
    #[allow(clippy::type_complexity)]
    fn build_callback(
        consumer: Arc<RecordingConsumer>,
    ) -> (
        QueueMessageCallback,
        Arc<DashMap<String, Tracked>>,
        Arc<DashMap<String, String>>,
    ) {
        let in_pipeline: Arc<DashMap<String, Tracked>> = Arc::new(DashMap::new());
        let app_index: Arc<DashMap<String, String>> = Arc::new(DashMap::new());
        let pending_delete = Arc::new(DashMap::new());

        let pipeline_key = "broker-msg-1".to_string();
        let app_message_id = "app-msg-1".to_string();

        // Simulate the manager pre-populating tracking maps before submit().
        let msg = Message {
            id: app_message_id.clone(),
            pool_code: String::new(),
            auth_token: None,
            signing_secret: None,
            mediation_type: fc_common::MediationType::HTTP,
            mediation_target: "http://localhost".to_string(),
            message_group_id: None,
            high_priority: false,
            dispatch_mode: fc_common::DispatchMode::Immediate,
            dispatch_mode_specified: true,
        };
        let in_flight = InFlightMessage::new(
            &msg,
            Some(pipeline_key.clone()),
            "queue-id".to_string(),
            None,
            "receipt-handle-xyz".to_string(),
        );
        in_pipeline.insert(pipeline_key.clone(), Tracked::new(in_flight.clone(), 1));
        app_index.insert(app_message_id.clone(), pipeline_key.clone());

        let cb = QueueMessageCallback {
            pipeline_key,
            app_message_id,
            queue_identifier: "queue-id".to_string(),
            generation: 1,
            admitted: in_flight,
            origin: consumer as Arc<dyn QueueConsumer>,
            origin_generation: 0,
            registry: Arc::new(ConsumerRegistry::default()),
            in_pipeline: in_pipeline.clone(),
            app_message_to_pipeline_key: app_index.clone(),
            pending_delete,
            completed: std::sync::atomic::AtomicBool::new(false),
        };
        (cb, in_pipeline, app_index)
    }

    #[tokio::test]
    async fn drop_without_resolution_clears_tracking_and_nacks() {
        let consumer = Arc::new(RecordingConsumer::default());
        let (cb, in_pipeline, app_index) = build_callback(consumer.clone());
        assert_eq!(in_pipeline.len(), 1);
        assert_eq!(app_index.len(), 1);

        // Drop without ack/nack — simulates panic / cancellation /
        // abandoned PoolTask.
        drop(cb);

        // Tracking maps cleared synchronously inside Drop.
        assert_eq!(
            in_pipeline.len(),
            0,
            "in_pipeline should be cleared on drop"
        );
        assert_eq!(app_index.len(), 0, "app index should be cleared on drop");

        // Fallback nack is fired via tokio::spawn — yield to let it run.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(
            consumer.nacks.load(AtomicOrdering::SeqCst),
            1,
            "fallback nack should have fired"
        );
        assert_eq!(consumer.acks.load(AtomicOrdering::SeqCst), 0);
    }

    #[tokio::test]
    async fn ack_then_drop_does_not_fire_fallback_nack() {
        let consumer = Arc::new(RecordingConsumer::default());
        let (cb, in_pipeline, _app_index) = build_callback(consumer.clone());

        cb.ack().await;
        assert_eq!(in_pipeline.len(), 0);
        assert_eq!(consumer.acks.load(AtomicOrdering::SeqCst), 1);

        // Drop happens implicitly here — should NOT fire a nack.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(
            consumer.nacks.load(AtomicOrdering::SeqCst),
            0,
            "no fallback nack after explicit ack"
        );
    }

    #[tokio::test]
    async fn nack_then_drop_does_not_fire_fallback_nack() {
        let consumer = Arc::new(RecordingConsumer::default());
        let (cb, in_pipeline, _app_index) = build_callback(consumer.clone());

        cb.nack(Some(15)).await;
        assert_eq!(in_pipeline.len(), 0);
        assert_eq!(consumer.nacks.load(AtomicOrdering::SeqCst), 1);

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // Total should still be 1 — Drop did not add a second nack.
        assert_eq!(
            consumer.nacks.load(AtomicOrdering::SeqCst),
            1,
            "no double-nack on drop after explicit nack"
        );
    }

    /// H12: when the entry under this callback's key belongs to a LATER
    /// admission (the original was reaped and a redelivery admitted anew),
    /// the old callback must not nack the message out from under the new
    /// copy, nor clear its entry.
    #[tokio::test]
    async fn stale_callback_leaves_a_later_admission_alone() {
        let consumer = Arc::new(RecordingConsumer::default());
        let (cb, in_pipeline, app_index) = build_callback(consumer.clone());
        // Re-admit: same key, new generation, fresher receipt.
        let mut newer = in_pipeline.get("broker-msg-1").unwrap().clone();
        newer.generation = 2;
        newer.msg.receipt_handle = "receipt-newer".to_string();
        in_pipeline.insert("broker-msg-1".to_string(), newer);

        cb.nack(Some(5)).await;
        assert_eq!(
            consumer.nacks.load(AtomicOrdering::SeqCst),
            0,
            "must not release the message the later admission owns"
        );
        assert_eq!(
            in_pipeline.get("broker-msg-1").unwrap().generation,
            2,
            "the later admission's entry must survive"
        );
        assert_eq!(app_index.len(), 1);
    }

    /// An ack whose entry was taken over acks with this copy's own receipt
    /// (the work did complete) and leaves the newer entry in place.
    #[tokio::test]
    async fn stale_callback_acks_with_its_own_receipt() {
        #[derive(Default)]
        struct Handles(parking_lot::Mutex<Vec<String>>);
        #[async_trait]
        impl QueueConsumer for Handles {
            fn identifier(&self) -> &str {
                "h"
            }
            async fn poll(&self, _: u32) -> QueueResult<Vec<QueuedMessage>> {
                Ok(vec![])
            }
            async fn ack(&self, h: &str) -> QueueResult<()> {
                self.0.lock().push(h.to_string());
                Ok(())
            }
            async fn nack(&self, _: &str, _: Option<u32>) -> QueueResult<()> {
                Ok(())
            }
            async fn extend_visibility(&self, _: &str, _: u32) -> QueueResult<()> {
                Ok(())
            }
            fn is_healthy(&self) -> bool {
                true
            }
            async fn stop(&self) {}
        }
        let handles = Arc::new(Handles::default());
        let (mut cb, in_pipeline, _app) = build_callback(Arc::new(RecordingConsumer::default()));
        cb.origin = handles.clone();
        let mut newer = in_pipeline.get("broker-msg-1").unwrap().clone();
        newer.generation = 2;
        newer.msg.receipt_handle = "receipt-newer".to_string();
        in_pipeline.insert("broker-msg-1".to_string(), newer);

        cb.ack().await;
        assert_eq!(*handles.0.lock(), vec!["receipt-handle-xyz".to_string()]);
        assert_eq!(in_pipeline.get("broker-msg-1").unwrap().generation, 2);
    }

    /// Go's `EnsureTracked`: an entry reaped while the message sat buffered
    /// is restored at dispatch; a DIFFERENT broker copy owning the app id
    /// answers false.
    #[tokio::test]
    async fn ensure_tracked_restores_a_reaped_entry_and_refuses_a_foreign_owner() {
        let consumer = Arc::new(RecordingConsumer::default());
        let (cb, in_pipeline, app_index) = build_callback(consumer.clone());
        in_pipeline.clear();
        app_index.clear();
        assert!(cb.ensure_tracked());
        assert_eq!(in_pipeline.get("broker-msg-1").unwrap().generation, 1);
        assert_eq!(app_index.get("app-msg-1").unwrap().value(), "broker-msg-1");

        // A different broker copy of the same app message owns it now.
        in_pipeline.clear();
        let foreign = cb.admitted.clone();
        in_pipeline.insert("other-broker-key".to_string(), Tracked::new(foreign, 9));
        app_index.insert("app-msg-1".to_string(), "other-broker-key".to_string());
        assert!(!cb.ensure_tracked());
        // Ours is not resurrected.
        assert!(!in_pipeline.contains_key("broker-msg-1"));
        // Drop without resolution must not touch the foreign entry either.
        drop(cb);
        assert!(in_pipeline.contains_key("other-broker-key"));
    }

    /// Go's `MarkRetrying` through the callback, and NATS's
    /// `HonoursDelayedReturn` false reaching the pool through it.
    #[tokio::test]
    async fn mark_retrying_and_honours_delayed_return_pass_through() {
        struct NoDelay;
        #[async_trait]
        impl QueueConsumer for NoDelay {
            fn identifier(&self) -> &str {
                "nats"
            }
            async fn poll(&self, _: u32) -> QueueResult<Vec<QueuedMessage>> {
                Ok(vec![])
            }
            async fn ack(&self, _: &str) -> QueueResult<()> {
                Ok(())
            }
            async fn nack(&self, _: &str, _: Option<u32>) -> QueueResult<()> {
                Ok(())
            }
            async fn extend_visibility(&self, _: &str, _: u32) -> QueueResult<()> {
                Ok(())
            }
            fn is_healthy(&self) -> bool {
                true
            }
            fn honours_delayed_return(&self) -> bool {
                false
            }
            async fn stop(&self) {}
        }
        let (mut cb, in_pipeline, _app) = build_callback(Arc::new(RecordingConsumer::default()));
        assert!(cb.honours_delayed_return());
        cb.origin = Arc::new(NoDelay);
        assert!(!cb.honours_delayed_return());

        cb.mark_retrying();
        cb.mark_retrying();
        let e = in_pipeline.get("broker-msg-1").unwrap();
        assert_eq!(e.attempts, 2);
        assert!(e.last_retry_at.is_some());
        drop(e);
        cb.ack().await;
    }

    /// A broker ack that never returns is bounded: the callback gives up
    /// after BROKER_OP_TIMEOUT, books the broker id for pending-delete, and
    /// clears tracking, instead of pinning the worker for ever.
    #[tokio::test(start_paused = true)]
    async fn a_hung_ack_is_bounded_and_booked_for_pending_delete() {
        struct HangingAck;
        #[async_trait]
        impl QueueConsumer for HangingAck {
            fn identifier(&self) -> &str {
                "hang"
            }
            async fn poll(&self, _: u32) -> QueueResult<Vec<QueuedMessage>> {
                Ok(vec![])
            }
            async fn ack(&self, _: &str) -> QueueResult<()> {
                std::future::pending().await
            }
            async fn nack(&self, _: &str, _: Option<u32>) -> QueueResult<()> {
                std::future::pending().await
            }
            async fn extend_visibility(&self, _: &str, _: u32) -> QueueResult<()> {
                Ok(())
            }
            fn is_healthy(&self) -> bool {
                true
            }
            async fn stop(&self) {}
        }
        let (mut cb, in_pipeline, _app) = build_callback(Arc::new(RecordingConsumer::default()));
        cb.origin = Arc::new(HangingAck);
        let pending = cb.pending_delete.clone();
        cb.ack().await; // paused clock auto-advances past the timeout
        assert!(in_pipeline.is_empty());
        assert_eq!(pending.len(), 1);
    }

    /// Regression: every pool's mediator the manager builds must record
    /// into the manager's single shared circuit breaker registry — not a
    /// private `CircuitBreakerRegistry::default()` per mediator. Otherwise
    /// a breaker tripping for an endpoint reached via one pool wouldn't
    /// protect other pools targeting the same endpoint, and the monitoring
    /// API (which reads the manager's registry) would show empty stats.
    /// Mirrors Java's single `circuitBreakers` shared by every
    /// `ProcessPool`.
    ///
    /// A `ProcessPool` itself no longer holds a circuit breaker registry at
    /// all (breaker admission/recording moved entirely into the mediator —
    /// see `mediator.rs`'s `Mediator::mediate` impl), so this test checks
    /// sharing at the mediator level: every mediator `build_mediator`
    /// produces (one per pool, on the production `PerPool` factory path)
    /// must expose the SAME registry Arc as `manager.circuit_breaker_registry()`.
    #[tokio::test]
    async fn pools_share_managers_circuit_breaker_registry() {
        // PerPool mediator path; no network occurs (we only record breaker
        // failures directly and never mediate a message).
        let manager = QueueManager::new(HttpMediatorConfig::production());

        // One `build_mediator()` call per pool the manager would create —
        // `get_or_create_pool` calls this internally for each new pool, so
        // calling it directly here pins the same wiring without needing a
        // live pool.
        let mediator_a = manager.build_mediator();
        let mediator_b = manager.build_mediator();

        let breakers_a = mediator_a
            .circuit_breaker_registry()
            .expect("HttpMediator must expose its circuit breaker registry");
        let breakers_b = mediator_b
            .circuit_breaker_registry()
            .expect("HttpMediator must expose its circuit breaker registry");

        // Pointer identity: both mediators and the manager hold the same Arc.
        assert!(
            Arc::ptr_eq(breakers_a, manager.circuit_breaker_registry()),
            "pool A's mediator must share the manager's circuit breaker registry"
        );
        assert!(
            Arc::ptr_eq(breakers_b, manager.circuit_breaker_registry()),
            "pool B's mediator must share the manager's circuit breaker registry"
        );

        // Behavioural cross-pool protection: failures recorded while pool A's
        // mediator mediates an endpoint trip the breaker, and pool B's
        // mediator targeting the same endpoint immediately sees it open.
        let endpoint = "http://shared.example/api";
        for _ in 0..20 {
            breakers_a.record_failure(endpoint);
        }
        assert_eq!(
            manager.circuit_breaker_registry().get_state(endpoint),
            Some(crate::CircuitBreakerState::Open),
            "failures recorded via one mediator must be visible through the manager's registry"
        );
        assert!(
            !breakers_b.allow_request(endpoint),
            "pool B's mediator must observe the breaker opened by pool A's failures"
        );
    }
}

/// R-13/R-16: `FC_ROUTER_STRICT_ROUTING` gate — `malformed_routing_reason`
/// and the private grouping helpers it depends on
/// (`group_by_pool`/`group_by_message_group`) are unit-tested directly here
/// since they're not `pub`.
#[cfg(test)]
mod routing_gate_tests {
    use super::*;
    use crate::mediator::HttpMediatorConfig;
    use fc_common::{DispatchMode, MediationType, Message, QueuedMessage};

    fn msg(
        id: &str,
        pool_code: &str,
        mode: DispatchMode,
        mode_specified: bool,
        group: Option<&str>,
    ) -> Message {
        Message {
            id: id.to_string(),
            pool_code: pool_code.to_string(),
            auth_token: None,
            signing_secret: None,
            mediation_type: MediationType::HTTP,
            mediation_target: "http://localhost/x".to_string(),
            message_group_id: group.map(|s| s.to_string()),
            high_priority: false,
            dispatch_mode: mode,
            dispatch_mode_specified: mode_specified,
        }
    }

    fn queued(
        id: &str,
        pool_code: &str,
        mode: DispatchMode,
        mode_specified: bool,
        group: Option<&str>,
    ) -> QueuedMessage {
        QueuedMessage {
            message: msg(id, pool_code, mode, mode_specified, group),
            receipt_handle: format!("rh-{id}"),
            broker_message_id: Some(format!("bh-{id}")),
            queue_identifier: "q".to_string(),
        }
    }

    #[test]
    fn malformed_reason_flags_empty_pool_code() {
        let m = msg("1", "", DispatchMode::Immediate, true, None);
        assert_eq!(malformed_routing_reason(&m), Some("empty pool_code"));
    }

    #[test]
    fn malformed_reason_flags_unspecified_dispatch_mode() {
        let m = msg("1", "POOL", DispatchMode::NextOnError, false, None);
        assert_eq!(malformed_routing_reason(&m), Some("empty dispatch_mode"));
    }

    #[test]
    fn malformed_reason_flags_ordered_mode_with_no_group() {
        let m = msg("1", "POOL", DispatchMode::NextOnError, true, None);
        assert_eq!(
            malformed_routing_reason(&m),
            Some("ordered dispatch_mode with no message_group_id")
        );
        let m2 = msg("1", "POOL", DispatchMode::BlockOnError, true, Some(""));
        assert_eq!(
            malformed_routing_reason(&m2),
            Some("ordered dispatch_mode with no message_group_id")
        );
    }

    #[test]
    fn malformed_reason_none_for_well_formed_messages() {
        assert_eq!(
            malformed_routing_reason(&msg("1", "POOL", DispatchMode::Immediate, true, None)),
            None
        );
        assert_eq!(
            malformed_routing_reason(&msg(
                "1",
                "POOL",
                DispatchMode::NextOnError,
                true,
                Some("grp")
            )),
            None
        );
        // pool_code empty check runs first, but a fully well-formed ordered
        // message must not trip on the group check either.
        assert_eq!(
            malformed_routing_reason(&msg(
                "1",
                "POOL",
                DispatchMode::BlockOnError,
                true,
                Some("grp")
            )),
            None
        );
    }

    /// R-13: two messages with no real ordered group (IMMEDIATE, or ordered
    /// with no group id) in the same batch must never land in the same
    /// NACK-cascade bucket — the deleted shared `"__DEFAULT__"` group
    /// anti-pattern would have merged them.
    #[test]
    fn group_by_message_group_never_shares_a_bucket_for_groupless_messages() {
        let manager = QueueManager::new(HttpMediatorConfig::dev());
        let a = queued("a", "POOL", DispatchMode::Immediate, true, None);
        let b = queued("b", "POOL", DispatchMode::NextOnError, true, None);
        let c = queued("c", "POOL", DispatchMode::BlockOnError, true, Some(""));

        let grouped = manager.group_by_message_group(vec![a, b, c]);
        assert_eq!(
            grouped.len(),
            3,
            "each groupless message must get its own bucket, not share one"
        );
    }

    /// A real ordered group (dispatch mode requires ordering + non-empty
    /// group id) is unaffected: messages sharing a real group id still land
    /// in the same bucket, preserving legitimate FIFO NACK-cascade behaviour.
    #[test]
    fn group_by_message_group_keeps_real_ordered_groups_together() {
        let manager = QueueManager::new(HttpMediatorConfig::dev());
        let a = queued("a", "POOL", DispatchMode::NextOnError, true, Some("g1"));
        let b = queued("b", "POOL", DispatchMode::NextOnError, true, Some("g1"));

        let grouped = manager.group_by_message_group(vec![a, b]);
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped.get("g1").map(|v| v.len()), Some(2));
    }

    /// R-13: an empty pool_code must warn exactly like an unknown one — it
    /// used to fall silently to DEFAULT-POOL with no warning at all.
    #[tokio::test]
    async fn group_by_pool_warns_identically_for_empty_and_unknown_pool_code() {
        let manager = QueueManager::new(HttpMediatorConfig::dev());
        let empty = queued("a", "", DispatchMode::Immediate, true, None);
        let unknown = queued("b", "NOPE", DispatchMode::Immediate, true, None);

        let by_pool = manager.group_by_pool(vec![empty, unknown]).await;
        assert_eq!(by_pool.len(), 1, "both fall back to the same default pool");
        assert_eq!(by_pool.values().next().unwrap().len(), 2);
        assert_eq!(
            manager.warning_service().warning_count(),
            2,
            "both the empty and the unknown pool_code should have warned identically"
        );
    }
}

#[cfg(test)]
mod g11_broker_scoping_tests {
    use super::*;
    use crate::mediator::HttpMediatorConfig;
    use fc_common::{DispatchMode, MediationType, Message, QueuedMessage};

    fn queued_on(id: &str, queue: &str, broker_id: &str, receipt: &str) -> QueuedMessage {
        QueuedMessage {
            message: Message {
                id: id.to_string(),
                pool_code: "POOL".to_string(),
                auth_token: None,
                signing_secret: None,
                mediation_type: MediationType::HTTP,
                mediation_target: "http://localhost/x".to_string(),
                message_group_id: None,
                high_priority: false,
                dispatch_mode: DispatchMode::Immediate,
                dispatch_mode_specified: true,
            },
            receipt_handle: receipt.to_string(),
            broker_message_id: Some(broker_id.to_string()),
            queue_identifier: queue.to_string(),
        }
    }

    /// G11: two different NATS streams' Nth message legitimately share the
    /// same bare broker id (`<streamSeq>:<consumerSeq>` is only unique
    /// within one stream). Registering both must produce two DISTINCT
    /// `in_pipeline` entries, and a redelivery on one queue must never
    /// touch the other queue's entry despite the shared bare id.
    ///
    /// Mutant check: reverting `pipeline_key`/`filter_duplicates`'s lookup
    /// to the bare broker id instead of `broker_scope_key(queue_identifier,
    /// broker_id)` makes queue B's registration silently overwrite queue
    /// A's `in_pipeline` entry (same map key) — the `len() == 2` assertion
    /// below catches that immediately — and, more subtly, sends A's
    /// redelivery down Check 1 for whichever queue's entry is currently at
    /// the bare key, corrupting the receipt handle the wrong queue would
    /// try to ack with — the final two assertions catch that (checked by
    /// hand against a bare-key revert while implementing this fix: the
    /// `b_entry.receipt_handle` assertion below is the one that fails).
    #[tokio::test]
    async fn same_broker_id_on_different_queues_gets_distinct_pipeline_entries() {
        let manager = QueueManager::new(HttpMediatorConfig::dev());

        let a = queued_on("msg-a", "BENCH1/router", "9", "receipt-a-v1");
        let b = queued_on("msg-b", "BENCH4/router", "9", "receipt-b-v1");

        // Register A, mimicking what route_batch does once filter_duplicates
        // admits a message as unique.
        let filtered_a = manager.filter_duplicates(vec![a.clone()]);
        assert_eq!(filtered_a.unique.len(), 1);
        let key_a = broker_scope_key(&a.queue_identifier, "9");
        manager.in_pipeline.insert(
            key_a.clone(),
            InFlightMessage::new(
                &a.message,
                a.broker_message_id.clone(),
                a.queue_identifier.clone(),
                None,
                a.receipt_handle.clone(),
            )
            .into(),
        );
        manager
            .app_message_to_pipeline_key
            .insert(a.message.id.clone(), key_a.clone());

        // Register B, on a different queue, sharing the bare broker id "9".
        let filtered_b = manager.filter_duplicates(vec![b.clone()]);
        assert_eq!(
            filtered_b.unique.len(),
            1,
            "queue BENCH4's message must not be mistaken for a redelivery \
             of queue BENCH1's, despite sharing the bare broker id \"9\""
        );
        let key_b = broker_scope_key(&b.queue_identifier, "9");
        manager.in_pipeline.insert(
            key_b.clone(),
            InFlightMessage::new(
                &b.message,
                b.broker_message_id.clone(),
                b.queue_identifier.clone(),
                None,
                b.receipt_handle.clone(),
            )
            .into(),
        );
        manager
            .app_message_to_pipeline_key
            .insert(b.message.id.clone(), key_b.clone());

        assert_eq!(
            manager.in_pipeline.len(),
            2,
            "two different queues' messages sharing a bare broker id must \
             not collapse into one tracker entry"
        );

        // A genuine redelivery of A (same queue, same broker id, fresh
        // receipt handle) must update ONLY A's entry.
        let a_redelivered = queued_on("msg-a", "BENCH1/router", "9", "receipt-a-v2");
        let filtered_redeliver = manager.filter_duplicates(vec![a_redelivered]);
        assert_eq!(
            filtered_redeliver.duplicates.len(),
            1,
            "A's redelivery must be recognised as a duplicate of A"
        );
        assert_eq!(filtered_redeliver.unique.len(), 0);

        let a_entry = manager.in_pipeline.get(&key_a).unwrap();
        assert_eq!(
            a_entry.receipt_handle, "receipt-a-v2",
            "A's receipt handle must be updated to the fresher redelivery"
        );
        drop(a_entry);

        let b_entry = manager.in_pipeline.get(&key_b).unwrap();
        assert_eq!(
            b_entry.receipt_handle, "receipt-b-v1",
            "B's entry must be untouched by A's redelivery — a bare-broker-id \
             key would route A's redelivery onto whichever queue's entry \
             happened to be at key \"9\" (the G11 cross-wiring bug)"
        );
    }
}
