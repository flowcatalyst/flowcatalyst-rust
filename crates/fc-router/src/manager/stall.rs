//! Stall detection/reporting and the stale-entry reaper: messages stuck in
//! `in_pipeline` past a threshold, once-per-episode warning dedup
//! (`report_stall`/`forget_resolved_stalls`), optional force-NACK, and the
//! periodic sweep that evicts stale `in_pipeline`/`pending_delete_broker_ids`
//! entries.

use std::time::Duration;

use chrono::Utc;
use tracing::{error, info, warn};

use fc_common::{StallConfig, StalledMessageInfo, WarningCategory, WarningSeverity};

use super::QueueManager;

impl QueueManager {
    /// Check for potential memory leaks (large in-pipeline maps)
    pub fn check_memory_health(&self) -> bool {
        let in_pipeline_size = self.in_pipeline.len();
        let threshold = 10000;

        if in_pipeline_size > threshold {
            warn!(
                in_pipeline_size = in_pipeline_size,
                threshold = threshold,
                "Potential memory leak detected - in_pipeline map is large"
            );
            return false;
        }

        true
    }

    /// Reap stale entries from in-memory tracking maps.
    ///
    /// In-flight entries follow Go's `InFlightTracker.Reap`: `max_age` is
    /// an IDLE bound measured on last-seen (refreshed by every broker
    /// redelivery), `max_age` × 8 is an absolute ceiling measured from
    /// admission, and a live in-place retry is exempt from the idle bound.
    /// Reaping on the admission time alone (as this did) dropped the entry
    /// of every delivery slower than 15 minutes; the next redelivery was
    /// then admitted as new — a second delivery of work in progress — and
    /// the first one's callback acked or nacked the second's entry.
    ///
    /// Also evicts `pending_delete_broker_ids` entries older than
    /// `pending_delete_max_age` (processed but never re-polled for deletion).
    pub fn reap_stale_entries(
        &self,
        max_age: Duration,
        pending_delete_max_age: Duration,
    ) -> (usize, usize) {
        // Skip iteration when maps are empty (common case — zero cost)
        if self.in_pipeline.is_empty() && self.pending_delete_broker_ids.is_empty() {
            return (0, 0);
        }

        let reaped_pipeline = self.reap_in_pipeline(max_age);

        // Reap stale pending_delete_broker_ids entries
        let reaped_pending = if self.pending_delete_broker_ids.is_empty() {
            0
        } else {
            let before = self.pending_delete_broker_ids.len();
            self.pending_delete_broker_ids
                .retain(|_, inserted_at| inserted_at.elapsed() < pending_delete_max_age);
            before - self.pending_delete_broker_ids.len()
        };

        if reaped_pending > 0 {
            info!(
                reaped = reaped_pending,
                max_age_seconds = pending_delete_max_age.as_secs(),
                "Reaped stale pending_delete_broker_ids entries"
            );
        }

        (reaped_pipeline, reaped_pending)
    }

    /// Go's `Reap` over `in_pipeline` (see [`Self::reap_stale_entries`]).
    /// Returns the number of entries removed.
    pub(super) fn reap_in_pipeline(&self, idle: Duration) -> usize {
        if self.in_pipeline.is_empty() {
            return 0;
        }
        let ceiling = idle * super::tracking::ABSOLUTE_MAX_AGE_FACTOR;
        let now = std::time::Instant::now();
        let stale: Vec<(String, u64)> = self
            .in_pipeline
            .iter()
            .filter(|e| e.value().should_reap(now, idle, ceiling))
            .map(|e| (e.key().clone(), e.value().generation))
            .collect();

        let mut reaped = 0;
        for (key, generation) in stale {
            // Generation-checked: a message admitted afresh under the same
            // key since the scan is not this entry.
            if let Some((_, entry)) = self
                .in_pipeline
                .remove_if(&key, |_, e| e.generation == generation)
            {
                self.app_message_to_pipeline_key
                    .remove_if(&entry.message_id, |_, k| *k == key);
                let past_ceiling = now.duration_since(entry.started_at) > ceiling;
                warn!(
                    message_id = %entry.message_id,
                    queue = %entry.queue_identifier,
                    pool_code = %entry.pool_code,
                    message_group_id = ?entry.message_group_id,
                    age_secs = entry.started_at.elapsed().as_secs(),
                    idle_secs = entry.last_seen.elapsed().as_secs(),
                    attempts = entry.attempts,
                    past_ceiling,
                    "Reaped in-flight entry — broker redelivery will retry"
                );
                reaped += 1;
            }
        }
        if reaped > 0 {
            warn!(
                reaped,
                idle_secs = idle.as_secs(),
                ceiling_secs = ceiling.as_secs(),
                "Reaped stale in-flight entries"
            );
        }
        reaped
    }

    /// Detect stalled messages that have been processing beyond the threshold.
    ///
    /// Returns a list of stalled message information for monitoring/alerting.
    pub fn detect_stalled_messages(&self) -> Vec<StalledMessageInfo> {
        if !self.stall_config.enabled {
            return Vec::new();
        }

        let threshold = self.stall_config.stall_threshold_seconds;
        let now = Utc::now();

        self.in_pipeline
            .iter()
            .filter(|entry| entry.value().elapsed_seconds() >= threshold)
            .map(|entry| {
                let msg = entry.value();
                StalledMessageInfo {
                    message_id: msg.message_id.clone(),
                    message_group_id: msg.message_group_id.clone(),
                    pool_code: msg.pool_code.clone(),
                    queue_identifier: msg.queue_identifier.clone(),
                    elapsed_seconds: msg.elapsed_seconds(),
                    detected_at: now,
                }
            })
            .collect()
    }

    /// X-04: emit a `Stall` warning through the shared `warning_service` for
    /// `message_id`, but only once per stall episode (mirrors Go's
    /// `StallDetector.report`). Returns `true` if a warning was actually
    /// recorded (first report of this episode); `false` if `message_id` was
    /// already reported and hasn't resolved since.
    fn report_stall(&self, message_id: &str, severity: WarningSeverity, message: String) -> bool {
        {
            let mut warned = self.stall_warned.lock();
            if !warned.insert(message_id.to_string()) {
                return false;
            }
        }
        self.warning_service.add_warning(
            WarningCategory::Stall,
            severity,
            message,
            "StallDetector".to_string(),
        );
        true
    }

    /// Drop dedup entries for message ids no longer present in `live` — once
    /// a message truly leaves the pipeline (acked / nacked / force-NACKed),
    /// a later stall of the same id must report again rather than being
    /// silenced for the life of the process (mirrors Go's
    /// `StallDetector.forgetResolved`).
    fn forget_resolved_stalls(&self, live: &std::collections::HashSet<String>) {
        let mut warned = self.stall_warned.lock();
        warned.retain(|id| live.contains(id));
    }

    /// Check for stalled messages and optionally force-NACK them.
    ///
    /// This method should be called periodically (e.g., every 30 seconds).
    /// It will:
    /// 1. Detect messages that have exceeded the stall threshold
    /// 2. Raise a `Stall` warning (store → notifier) for each, once per
    ///    episode
    /// 3. If force_nack_stalled is enabled, NACK messages exceeding the force_nack_after_seconds threshold
    ///
    /// Returns the number of messages that were force-NACKed.
    pub async fn check_and_handle_stalled_messages(&self) -> usize {
        if !self.stall_config.enabled {
            return 0;
        }

        let stalled = self.detect_stalled_messages();

        // X-04: forget dedup entries for messages no longer in the pipeline
        // at all (acked/nacked since the last tick), so a later stall of the
        // same id reports again instead of being silenced forever. Run this
        // even when nothing is currently stalled, so resolved entries don't
        // linger in `stall_warned`.
        let live: std::collections::HashSet<String> = self
            .in_pipeline
            .iter()
            .map(|entry| entry.value().message_id.clone())
            .collect();
        self.forget_resolved_stalls(&live);

        if stalled.is_empty() {
            return 0;
        }

        // Report stalled messages once per message per episode (X-04): both
        // the operational log line and the WarningService entry (which now
        // drives /warnings, health's active-warning count, and the
        // notifier) are gated by `report_stall`, so a handful of
        // long-running deliveries doing their job can't push the router
        // into Warning/Degraded purely by being re-reported every tick.
        // Go's two populations: a message a worker is retrying in place is
        // reported as retrying and is never force-NACKed (that would hand
        // the broker a second copy while the retry still runs); the rest
        // are stalled.
        let retrying: std::collections::HashSet<String> = self
            .in_pipeline
            .iter()
            .filter(|e| e.value().attempts > 0)
            .map(|e| e.value().message_id.clone())
            .collect();

        for msg in &stalled {
            if retrying.contains(&msg.message_id) {
                if self.report_stall(
                    &msg.message_id,
                    WarningSeverity::Warn,
                    format!(
                        "Message {} has been retrying in-pipeline for {}s in pool {}",
                        msg.message_id, msg.elapsed_seconds, msg.pool_code
                    ),
                ) {
                    warn!(
                        message_id = %msg.message_id,
                        pool_code = %msg.pool_code,
                        elapsed_seconds = msg.elapsed_seconds,
                        "Message retrying in-pipeline past the stall threshold"
                    );
                }
                continue;
            }
            let reported = self.report_stall(
                &msg.message_id,
                WarningSeverity::Warn,
                format!(
                    "Message {} stalled for {}s in pool {}",
                    msg.message_id, msg.elapsed_seconds, msg.pool_code
                ),
            );
            if reported {
                warn!(
                    message_id = %msg.message_id,
                    message_group_id = ?msg.message_group_id,
                    pool_code = %msg.pool_code,
                    queue_identifier = %msg.queue_identifier,
                    elapsed_seconds = msg.elapsed_seconds,
                    "Stalled message detected - processing time exceeds threshold"
                );
            }
        }

        // If force-NACK is not enabled, just return the count of detected stalls
        if !self.stall_config.force_nack_stalled {
            info!(
                stalled_count = stalled.len(),
                threshold_seconds = self.stall_config.stall_threshold_seconds,
                "Stalled messages detected (force-NACK disabled)"
            );
            return 0;
        }

        // Force-NACK messages that have exceeded the force_nack_after_seconds threshold
        let force_threshold = self.stall_config.force_nack_after_seconds;
        let nack_delay = self.stall_config.nack_delay_seconds;

        let mut force_nacked = 0;

        for msg in &stalled {
            if msg.elapsed_seconds >= force_threshold && !retrying.contains(&msg.message_id) {
                // `in_pipeline` is keyed by pipeline_key (the broker
                // message id, queue-scoped — G11), not by the application
                // `message_id` `StalledMessageInfo` carries — resolve the
                // real key through `app_message_to_pipeline_key` first,
                // same as `force_ack_in_flight`/`is_in_flight_by_app_id`.
                let Some(pipeline_key) = self
                    .app_message_to_pipeline_key
                    .get(&msg.message_id)
                    .map(|e| e.value().clone())
                else {
                    continue;
                };
                // Get the in-flight message to get the receipt handle
                if let Some(in_flight) = self.in_pipeline.get(&pipeline_key) {
                    let receipt_handle = in_flight.receipt_handle.clone();
                    let queue_id = in_flight.queue_identifier.clone();
                    drop(in_flight); // Release the lock before async call

                    // G10: resolve by identifier() — the broker identity —
                    // never the config queue name.
                    if let Some(consumer) = self.consumers.resolve(&queue_id, 0) {
                        warn!(
                            message_id = %msg.message_id,
                            elapsed_seconds = msg.elapsed_seconds,
                            force_threshold_seconds = force_threshold,
                            "Force-NACKing stalled message"
                        );

                        if let Err(e) = consumer.nack(&receipt_handle, Some(nack_delay)).await {
                            error!(
                                message_id = %msg.message_id,
                                error = %e,
                                "Failed to force-NACK stalled message"
                            );
                        } else {
                            // Remove from pipeline since we've force-NACKed
                            self.in_pipeline.remove(&pipeline_key);
                            self.app_message_to_pipeline_key.remove(&msg.message_id);
                            force_nacked += 1;
                        }
                    }
                }
            }
        }

        if force_nacked > 0 {
            info!(
                force_nacked = force_nacked,
                total_stalled = stalled.len(),
                "Force-NACKed stalled messages"
            );
        }

        force_nacked
    }

    /// Get stall detection configuration
    pub fn stall_config(&self) -> &StallConfig {
        &self.stall_config
    }

    /// Update stall detection configuration at runtime
    pub fn update_stall_config(&mut self, config: StallConfig) {
        info!(
            enabled = config.enabled,
            stall_threshold_seconds = config.stall_threshold_seconds,
            force_nack_stalled = config.force_nack_stalled,
            force_nack_after_seconds = config.force_nack_after_seconds,
            "Updating stall detection configuration"
        );
        self.stall_config = config;
    }
}

/// X-04: `check_and_handle_stalled_messages` now raises `Stall` warnings
/// through `warning_service` (store → notifier) instead of only
/// `tracing::warn!`, so the once-per-episode dedup (`report_stall` /
/// `forget_resolved_stalls`, mirroring Go's `StallDetector`) is what keeps a
/// long-running-but-legitimate delivery from re-reporting every tick and
/// pushing the router into Warning/Degraded on warning volume alone.
#[cfg(test)]
mod stall_warning_tests {
    use super::*;
    use crate::mediator::HttpMediatorConfig;
    use fc_common::{DispatchMode, InFlightMessage, MediationType, Message};
    use std::time::Instant;

    fn stalled_in_flight(message_id: &str) -> InFlightMessage {
        let msg = Message {
            id: message_id.to_string(),
            pool_code: "POOL".to_string(),
            auth_token: None,
            signing_secret: None,
            mediation_type: MediationType::HTTP,
            mediation_target: "http://localhost/x".to_string(),
            message_group_id: None,
            high_priority: false,
            dispatch_mode: DispatchMode::Immediate,
            dispatch_mode_specified: true,
        };
        let mut in_flight = InFlightMessage::new(
            &msg,
            Some(format!("bh-{message_id}")),
            "queue".to_string(),
            None,
            "rh".to_string(),
        );
        // Backdate well past any threshold used below.
        in_flight.started_at = Instant::now() - Duration::from_secs(10);
        in_flight
    }

    fn manager_with_stall_threshold(secs: u64) -> super::QueueManager {
        super::QueueManager::builder(HttpMediatorConfig::dev())
            .stall_config(StallConfig {
                enabled: true,
                stall_threshold_seconds: secs,
                force_nack_stalled: false,
                ..StallConfig::default()
            })
            .build()
    }

    #[tokio::test]
    async fn stall_warning_reported_once_per_episode() {
        let manager = manager_with_stall_threshold(5);
        manager
            .in_pipeline
            .insert("bh-msg-1".to_string(), stalled_in_flight("msg-1").into());

        // Simulate several detector ticks against the same still-stalled message.
        manager.check_and_handle_stalled_messages().await;
        manager.check_and_handle_stalled_messages().await;
        manager.check_and_handle_stalled_messages().await;

        let stall_warnings = manager
            .warning_service()
            .get_warnings_by_category(WarningCategory::Stall);
        assert_eq!(
            stall_warnings.len(),
            1,
            "the same stalled message must raise exactly one Stall warning across repeated ticks"
        );
        assert_eq!(stall_warnings[0].severity, WarningSeverity::Warn);
    }

    #[tokio::test]
    async fn stall_warning_reports_again_after_resolving_and_restalling() {
        let manager = manager_with_stall_threshold(5);
        manager
            .in_pipeline
            .insert("bh-msg-1".to_string(), stalled_in_flight("msg-1").into());

        manager.check_and_handle_stalled_messages().await;
        assert_eq!(
            manager
                .warning_service()
                .get_warnings_by_category(WarningCategory::Stall)
                .len(),
            1
        );

        // Message resolves (acked/nacked/force-NACKed) — leaves the
        // pipeline entirely. A tick with nothing stalled must still prune
        // the dedup entry.
        manager.in_pipeline.remove("bh-msg-1");
        manager.check_and_handle_stalled_messages().await;

        // The same message id stalls again later (e.g. redelivered) — this
        // must report again rather than staying silenced for the life of
        // the process.
        manager
            .in_pipeline
            .insert("bh-msg-1".to_string(), stalled_in_flight("msg-1").into());
        manager.check_and_handle_stalled_messages().await;

        assert_eq!(
            manager
                .warning_service()
                .get_warnings_by_category(WarningCategory::Stall)
                .len(),
            2,
            "a fresh stall of a previously-resolved message id must report again"
        );
    }

    /// H12: the reaper ages on last-seen — an entry admitted 20 minutes ago
    /// whose broker keeps redelivering it (last seen just now) stays; one
    /// nobody has seen for 16 minutes goes; nothing outlives the ceiling.
    #[test]
    fn reaper_ages_on_last_seen_with_an_absolute_ceiling() {
        let manager = manager_with_stall_threshold(5);
        let now = Instant::now();
        let mut live: super::super::tracking::Tracked = stalled_in_flight("live").into();
        live.msg.started_at = now - Duration::from_secs(20 * 60);
        live.last_seen = now;
        let mut idle: super::super::tracking::Tracked = stalled_in_flight("idle").into();
        idle.msg.started_at = now - Duration::from_secs(20 * 60);
        idle.last_seen = now - Duration::from_secs(16 * 60);
        let mut ancient: super::super::tracking::Tracked = stalled_in_flight("ancient").into();
        ancient.msg.started_at = now - Duration::from_secs(3 * 60 * 60);
        ancient.last_seen = now;
        manager.in_pipeline.insert("k-live".into(), live);
        manager.in_pipeline.insert("k-idle".into(), idle);
        manager.in_pipeline.insert("k-ancient".into(), ancient);

        let (reaped, _) =
            manager.reap_stale_entries(Duration::from_secs(15 * 60), Duration::from_secs(60));
        assert_eq!(reaped, 2);
        assert!(manager.in_pipeline.contains_key("k-live"));
    }

    /// Go: a message being retried in place is reported as retrying and is
    /// never force-NACKed.
    #[tokio::test]
    async fn retrying_message_is_never_force_nacked() {
        let manager = manager_with_force_nack();
        let consumer = Arc::new(RecordingConsumer::default());
        let rc = manager.new_running_consumer(consumer.clone(), "STREAM1".to_string(), None);
        manager.consumers.insert(rc);
        let mut in_flight: super::super::tracking::Tracked = stalled_in_flight("msg-r").into();
        in_flight.msg.queue_identifier = "STREAM1/router".to_string();
        in_flight.mark_retrying();
        manager.in_pipeline.insert("k-r".to_string(), in_flight);
        manager
            .app_message_to_pipeline_key
            .insert("msg-r".to_string(), "k-r".to_string());

        assert_eq!(manager.check_and_handle_stalled_messages().await, 0);
        assert_eq!(consumer.nacks.load(AtomicOrdering::SeqCst), 0);
        assert!(manager.in_pipeline.contains_key("k-r"));
        let warnings = manager
            .warning_service()
            .get_warnings_by_category(WarningCategory::Stall);
        assert!(warnings[0].message.contains("retrying"));
    }

    // --- G10: force-NACK must resolve the consumer by identifier(), not
    // the config queue name, and must resolve the in-pipeline entry via
    // app_message_to_pipeline_key rather than the bare application id. ---

    use async_trait::async_trait;
    use fc_common::QueuedMessage;
    use fc_queue::{QueueConsumer, Result as QueueResult};
    use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};
    use std::sync::Arc;

    /// Records nack() calls; a broker-native `identifier()` that deliberately
    /// differs from any config queue name a test might otherwise key things
    /// by (G10's whole point).
    #[derive(Default)]
    struct RecordingConsumer {
        nacks: AtomicU32,
    }

    #[async_trait]
    impl QueueConsumer for RecordingConsumer {
        fn identifier(&self) -> &str {
            "STREAM1/router"
        }
        async fn poll(&self, _: u32) -> QueueResult<Vec<QueuedMessage>> {
            Ok(vec![])
        }
        async fn ack(&self, _: &str) -> QueueResult<()> {
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

    fn manager_with_force_nack() -> super::QueueManager {
        super::QueueManager::builder(HttpMediatorConfig::dev())
            .stall_config(StallConfig {
                enabled: true,
                stall_threshold_seconds: 5,
                force_nack_stalled: true,
                force_nack_after_seconds: 0, // anything stalled immediately qualifies
                nack_delay_seconds: 5,
            })
            .build()
    }

    /// Pins: force-NACK actually reaches the consumer and clears the
    /// tracker entry, when the consumer is registered under its own
    /// `identifier()` — a broker-native id ("STREAM1/router") that is
    /// *not* the config queue name — and the in-pipeline entry is keyed by
    /// a broker-derived pipeline_key ("scoped-key-1") that is *not* the
    /// bare application message id ("msg-1").
    ///
    /// Mutant check A (G10): resolving by config name instead of by
    /// identifier — the consumer is registered under the name "STREAM1" —
    /// finds nothing, `nacks` stays 0, this test fails.
    ///
    /// Mutant check B (adjacent pipeline_key defect): resolving
    /// `self.in_pipeline.get(&msg.message_id)` (bare app id "msg-1")
    /// instead of resolving through `app_message_to_pipeline_key` first —
    /// "msg-1" is never a key in `in_pipeline` (only "scoped-key-1" is),
    /// so the lookup finds nothing, `nacks` stays 0, this test fails.
    #[tokio::test]
    async fn force_nack_resolves_consumer_by_identifier_and_pipeline_key_by_app_id_index() {
        let manager = manager_with_force_nack();
        let consumer = Arc::new(RecordingConsumer::default());

        // Registered under a config name ("STREAM1") that differs from its
        // identifier() — a lookup by name for "STREAM1/router" must miss.
        let rc = manager.new_running_consumer(consumer.clone(), "STREAM1".to_string(), None);
        manager.consumers.insert(rc);

        let mut in_flight = stalled_in_flight("msg-1");
        in_flight.queue_identifier = "STREAM1/router".to_string();
        manager
            .in_pipeline
            .insert("scoped-key-1".to_string(), in_flight.into());
        manager
            .app_message_to_pipeline_key
            .insert("msg-1".to_string(), "scoped-key-1".to_string());

        let force_nacked = manager.check_and_handle_stalled_messages().await;

        assert_eq!(
            consumer.nacks.load(AtomicOrdering::SeqCst),
            1,
            "the registered consumer's nack() must have been called exactly once"
        );
        assert_eq!(force_nacked, 1);
        assert!(
            !manager.in_pipeline.contains_key("scoped-key-1"),
            "the in-pipeline entry must be cleared once force-NACKed"
        );
        assert!(
            manager.app_message_to_pipeline_key.get("msg-1").is_none(),
            "the app-id index entry must be cleared once force-NACKed"
        );
    }
}
