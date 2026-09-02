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
    BatchMessage, InFlightMessage, MessageCallback, QueuedMessage, WarningCategory,
    WarningSeverity,
};
use fc_queue::QueueConsumer;

use crate::error::RouterError;
use crate::Result;

use super::QueueManager;

/// Callback that the pool worker calls directly when processing completes.
/// Reads the latest receipt handle from in_pipeline (may have been swapped by
/// redelivery), performs the SQS operation, then cleans up tracking.
/// No spawned task, no channel — mirrors the TS closure pattern.
///
/// **Drop safety.** If this callback is dropped without `ack()` or `nack()`
/// being called (panic during mediation, runtime cancellation, abandoned
/// queue task on early drain-task exit, …) the `Drop` impl guarantees:
///
/// 1. The entry is removed from `in_pipeline` and
///    `app_message_to_pipeline_key`. Without this cleanup, SQS redeliveries
///    of the same `broker_message_id` would be silently swallowed by
///    `filter_duplicates` Phase 1 (Check 1) and the message would stick
///    until the SQS message retention period expires — observed in
///    production as "thousands of messages stuck".
/// 2. A best-effort `nack` is fired via `tokio::spawn` so SQS releases the
///    visibility timeout sooner than its default. Failures here are
///    swallowed; the natural visibility timeout is the eventual safety net.
struct QueueMessageCallback {
    pipeline_key: String,
    app_message_id: String,
    consumer: Arc<dyn QueueConsumer + Send + Sync>,
    in_pipeline: Arc<DashMap<String, InFlightMessage>>,
    app_message_to_pipeline_key: Arc<DashMap<String, String>>,
    pending_delete: Arc<DashMap<String, Instant>>,
    /// Set to true the moment `ack()` or `nack()` is entered. The `Drop`
    /// impl checks this and only fires fallback cleanup if no resolution
    /// happened. AcqRel ordering: the load in Drop must observe stores from
    /// any thread that called ack/nack.
    completed: std::sync::atomic::AtomicBool,
}

impl QueueMessageCallback {
    /// Common cleanup: drop the in-memory tracking entries so future
    /// redeliveries of this `broker_message_id` flow through Phase 2 again
    /// instead of being silently swallowed as duplicates.
    fn cleanup_tracking(&self) {
        self.in_pipeline.remove(&self.pipeline_key);
        self.app_message_to_pipeline_key
            .remove(&self.app_message_id);
    }
}

#[async_trait::async_trait]
impl MessageCallback for QueueMessageCallback {
    async fn ack(&self) {
        // Mark resolved BEFORE doing any await so the Drop impl knows we
        // owned the resolution even if a panic happens mid-await.
        self.completed
            .store(true, std::sync::atomic::Ordering::Release);

        // Read latest receipt handle (may have been updated by redelivery)
        let (handle, broker_id) = self
            .in_pipeline
            .get(&self.pipeline_key)
            .map(|e| (e.receipt_handle.clone(), e.broker_message_id.clone()))
            .unwrap_or_default();

        if handle.is_empty() {
            error!(
                pipeline_key = %self.pipeline_key,
                app_message_id = %self.app_message_id,
                "ACK skipped — no receipt handle in in_pipeline (entry may have been reaped)"
            );
        } else {
            if let Err(e) = self.consumer.ack(&handle).await {
                // ACK failed — add to pending_delete BEFORE removing from in_pipeline
                if let Some(ref bid) = broker_id {
                    warn!(
                        broker_message_id = %bid,
                        app_message_id = %self.app_message_id,
                        error = %e,
                        "ACK failed (receipt handle likely expired) - adding to pending delete"
                    );
                    self.pending_delete.insert(bid.clone(), Instant::now());
                } else {
                    error!(
                        app_message_id = %self.app_message_id,
                        error = %e,
                        "ACK failed and no broker message ID to track for pending delete"
                    );
                }
            }
        }

        // Clean up tracking AFTER SQS operation
        self.cleanup_tracking();
    }

    async fn nack(&self, delay_seconds: Option<u32>) {
        // Mark resolved BEFORE doing any await; see ack() above.
        self.completed
            .store(true, std::sync::atomic::Ordering::Release);

        let handle = self
            .in_pipeline
            .get(&self.pipeline_key)
            .map(|e| e.receipt_handle.clone())
            .unwrap_or_default();

        if handle.is_empty() {
            error!(
                pipeline_key = %self.pipeline_key,
                app_message_id = %self.app_message_id,
                "NACK skipped — no receipt handle in in_pipeline (entry may have been reaped)"
            );
        } else {
            let _ = self.consumer.nack(&handle, delay_seconds).await;
        }

        // Clean up tracking AFTER SQS operation
        self.cleanup_tracking();
    }
}

impl Drop for QueueMessageCallback {
    fn drop(&mut self) {
        // Fast path: ack() or nack() ran, no fallback needed.
        if self.completed.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }

        // The callback was dropped without resolution. Most likely causes:
        //   • mediator panicked mid-mediation
        //   • tokio task was cancelled
        //   • drain task exited early leaving queued PoolTasks abandoned
        //
        // Always clear the in-memory tracking so SQS redeliveries are not
        // silently swallowed. Fire a best-effort nack so the message
        // returns to the queue sooner than its full visibility timeout.

        let pipeline_key = self.pipeline_key.clone();
        let app_message_id = self.app_message_id.clone();

        // Snapshot the current receipt handle before we yank the entry.
        let handle = self
            .in_pipeline
            .get(&pipeline_key)
            .map(|e| e.receipt_handle.clone())
            .unwrap_or_default();

        // Synchronous cleanup of tracking — never deferred.
        self.cleanup_tracking();

        warn!(
            pipeline_key = %pipeline_key,
            app_message_id = %app_message_id,
            "Callback dropped without ack/nack — fallback cleanup ran (likely mediator panic or task cancel)"
        );

        if !handle.is_empty() {
            // Best-effort nack on a detached task. If we can't get a tokio
            // handle (e.g. shutting down), the SQS visibility timeout will
            // eventually redeliver and processing will retry.
            if let Ok(rt) = tokio::runtime::Handle::try_current() {
                let consumer = self.consumer.clone();
                rt.spawn(async move {
                    let _ = consumer.nack(&handle, Some(10)).await;
                });
            }
        }
    }
}

/// Reports why `msg` is malformed under strict routing
/// (`FC_ROUTER_STRICT_ROUTING`; see [`QueueManager::set_strict_routing`]),
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
    /// Route a batch of messages from a consumer poll
    pub async fn route_batch(
        &self,
        messages: Vec<QueuedMessage>,
        consumer: Arc<dyn QueueConsumer>,
    ) -> Result<()> {
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
            return Ok(());
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
            let should_delete = msg
                .broker_message_id
                .as_ref()
                .map(|broker_id| self.pending_delete_broker_ids.remove(broker_id).is_some())
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
            return Ok(());
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
        let well_formed = if self.strict_routing.load(Ordering::SeqCst) {
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

            // Check pool capacity for ALL messages in this pool
            let available = pool.available_capacity();
            if available < pool_messages.len() {
                warn!(
                    pool_code = %pool_code,
                    available = available,
                    requested = pool_messages.len(),
                    "Pool at capacity, deferring all messages for this pool"
                );
                self.warning_service.add_warning(
                    WarningCategory::QueueHealth,
                    WarningSeverity::Warn,
                    format!(
                        "Pool [{}] queue full, deferring {} messages from batch",
                        pool_code,
                        pool_messages.len()
                    ),
                    "QueueManager".to_string(),
                );
                // Defer concurrently - capacity limits are not errors
                let defer_futs: Vec<_> = pool_messages
                    .iter()
                    .map(|msg| {
                        let consumer = consumer.clone();
                        let handle = msg.receipt_handle.clone();
                        async move {
                            let _ = consumer.defer(&handle, Some(5)).await;
                        }
                    })
                    .collect();
                future::join_all(defer_futs).await;
                continue;
            }

            // Note: Rate limiting is now handled inside the pool worker (blocking wait)
            // Messages stay in pool queue instead of being deferred back to SQS

            // Phase 3: Group by messageGroupId for FIFO ordering enforcement
            // This mirrors Java's messagesByGroup logic in routeMessageBatch
            let messages_by_group = self.group_by_message_group(pool_messages);

            for (group_id, group_messages) in messages_by_group {
                let mut nack_remaining = false;

                for msg in group_messages {
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

                    // Use broker_message_id as pipeline key (mirrors Java's sqsMessageId usage)
                    // Fall back to a composite key if broker_message_id is not available
                    let pipeline_key = msg.broker_message_id.clone().unwrap_or_else(|| {
                        format!("fallback:{}:{}", msg.queue_identifier, msg.message.id)
                    });

                    let receipt_handle = msg.receipt_handle.clone();

                    // Track in pipeline with receipt handle
                    let in_flight = InFlightMessage::new(
                        &msg.message,
                        msg.broker_message_id.clone(),
                        msg.queue_identifier.clone(),
                        Some(Arc::clone(&batch_id)),
                        msg.receipt_handle.clone(),
                    );
                    self.in_pipeline.insert(pipeline_key.clone(), in_flight);

                    // Track app message ID -> pipeline key for requeue detection
                    self.app_message_to_pipeline_key
                        .insert(app_message_id.clone(), pipeline_key.clone());

                    // Create callback — pool worker calls this directly, no spawned task
                    let callback = QueueMessageCallback {
                        pipeline_key: pipeline_key.clone(),
                        app_message_id: app_message_id.clone(),
                        consumer: consumer.clone(),
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

        Ok(())
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
                if let Some(mut entry) = self.in_pipeline.get_mut(broker_msg_id) {
                    // Update receipt handle with the new one from the redelivered message
                    // This ensures when processing completes, ACK uses the valid (latest) receipt handle
                    if entry.receipt_handle != msg.receipt_handle {
                        debug!(
                            message_id = %msg.message.id,
                            broker_message_id = %broker_msg_id,
                            "Updating receipt handle for redelivered message (visibility timeout)"
                        );
                        entry.receipt_handle = msg.receipt_handle.clone();
                        // Also update broker_message_id in case it was a fallback key
                        if entry.broker_message_id.is_none() {
                            entry.broker_message_id = Some(broker_msg_id.clone());
                        }
                    }
                    let pipeline_key = broker_msg_id.clone();
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
                // If they're the same, it would have been caught by the check above
                if let Some(ref new_broker_id) = msg.broker_message_id {
                    if *new_broker_id != existing_key {
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
                    // Update receipt handle for redelivery
                    if entry.receipt_handle != msg.receipt_handle {
                        debug!(
                            message_id = %msg.message.id,
                            "Updating receipt handle for redelivered message"
                        );
                        entry.receipt_handle = msg.receipt_handle.clone();
                    }
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
        Arc<DashMap<String, InFlightMessage>>,
        Arc<DashMap<String, String>>,
    ) {
        let in_pipeline: Arc<DashMap<String, InFlightMessage>> = Arc::new(DashMap::new());
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
        in_pipeline.insert(pipeline_key.clone(), in_flight);
        app_index.insert(app_message_id.clone(), pipeline_key.clone());

        let cb = QueueMessageCallback {
            pipeline_key,
            app_message_id,
            consumer: consumer as Arc<dyn QueueConsumer + Send + Sync>,
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
