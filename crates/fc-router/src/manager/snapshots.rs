//! Read-side views: pool stats/capacity, the operator dashboard snapshots
//! (mediating entries, blocked groups, group-flush suppressions), the
//! force-ack override, and in-flight message lookups.

use std::collections::HashMap;
use std::sync::Arc;

use tracing::warn;
use utoipa::ToSchema;

use fc_common::PoolStats;

use crate::event_counters::{ConsumerEventCounters, ConsumerEventSnapshot, PoolEventSnapshot};
use crate::pool::ProcessPool;

use super::routing::ack_bounded;
use super::QueueManager;
use crate::flight_recorder::EventContext;
use crate::flight_recorder::EventKind;
use crate::flight_recorder::Facts;
use crate::group_flush::GroupSuppression;
use crate::pool::BufferedMessage;
use crate::pool::GroupInfo;
use crate::pool::MediatingEntry;
use std::cmp::Reverse;
use std::time::Instant;

impl QueueManager {
    /// Check if any pool has capacity to accept messages.
    /// Used to gate SQS polling — avoids a hot poll-defer loop when all pools are full.
    /// Only considers `Active` entries — a draining pool is never a routing
    /// candidate (see the `pools` field's doc comment).
    pub(super) fn has_pool_capacity(&self) -> bool {
        let mut saw_active = false;
        for entry in self.pools.iter() {
            saw_active = true;
            if entry.value().available_capacity() > 0 {
                return true;
            }
        }
        !saw_active
    }

    /// Whether consumer `rc` should poll (Go: `hasCapacityFor`): some pool
    /// its last batch fed has room — judged by this queue's own traffic,
    /// not "any pool anywhere has room", which let an idle pool elsewhere
    /// keep a consumer polling into pools that were all full — or, when
    /// they are all full, its outstanding capacity deferrals are still
    /// under the deferral budget: the queue may have other pools' traffic
    /// queued behind what it last saw, and the only way to reach it is to
    /// poll and defer what is in the way. With no destinations known yet
    /// (first poll) it falls back to any pool having room. With no pools at
    /// all it polls: pools are created on demand here.
    pub(super) fn has_capacity_for(&self, rc: &super::RunningConsumer) -> bool {
        let dests = rc.dest_pools();
        let dest_has_room = if dests.is_empty() {
            self.has_pool_capacity()
        } else {
            let mut room = false;
            for code in &dests {
                match self.active_pool(code) {
                    // The pool went away under a reconfigure: re-learn
                    // where this queue's traffic goes.
                    None => {
                        room = self.has_pool_capacity();
                        break;
                    }
                    Some(pool) if pool.available_capacity() > 0 => {
                        room = true;
                        break;
                    }
                    Some(_) => {}
                }
            }
            room
        };
        dest_has_room || rc.deferrals_outstanding(Instant::now()) < self.deferral_budget
    }

    /// Get statistics for all active pools.
    pub fn get_pool_stats(&self) -> Vec<PoolStats> {
        self.pools
            .iter()
            .map(|entry| entry.value().get_stats())
            .collect()
    }

    /// Event-time counters of every active pool, by pool code, for the
    /// Prometheus surface.
    pub fn pool_event_snapshots(&self) -> HashMap<String, PoolEventSnapshot> {
        self.pools
            .iter()
            .map(|e| (e.key().clone(), e.value().event_snapshot()))
            .collect()
    }

    /// Poll counters of every queue that has been polled, by queue name.
    pub fn consumer_event_snapshots(&self) -> Vec<(String, ConsumerEventSnapshot)> {
        self.consumer_events
            .iter()
            .map(|e| (e.key().clone(), e.value().snapshot()))
            .collect()
    }

    /// The counters for `queue`, created on first use.
    pub(super) fn consumer_events_for(&self, queue: &str) -> Arc<ConsumerEventCounters> {
        if let Some(events) = self.consumer_events.get(queue) {
            return events.clone();
        }
        self.consumer_events
            .entry(queue.to_string())
            .or_default()
            .clone()
    }

    /// Every pool this manager is tracking, active or still draining after
    /// removal. Mirrors Go's `Manager.AllPools`: a pool
    /// still finishing an asynchronous removal-drain (X-11) can still be
    /// holding buffered groups, live group-flush suppressions, or
    /// in-worker deliveries worth showing on the operator dashboard, so the
    /// "Mediating"/"blocked groups"/"group flushes" views traverse both, so a
    /// draining predecessor's still-buffered/in-flight work never silently
    /// drops off the dashboard.
    fn all_pools(&self) -> Vec<Arc<ProcessPool>> {
        let mut all: Vec<Arc<ProcessPool>> = self.pools.iter().map(|e| e.value().clone()).collect();
        all.extend(self.draining_pools.lock().iter().cloned());
        all
    }

    /// Every message currently inside a worker, across every pool this
    /// manager is tracking — the operator "Mediating" dashboard view.
    pub fn mediating_snapshot(&self) -> Vec<MediatingEntry> {
        self.all_pools()
            .iter()
            .flat_map(|p| p.mediating_snapshot())
            .collect()
    }

    /// Where `message_id` is buffered behind its group's head, in any pool.
    pub fn find_buffered(&self, message_id: &str) -> Option<BufferedMessage> {
        self.all_pools()
            .iter()
            .find_map(|p| p.find_buffered(message_id))
    }

    /// `group`'s buffer on every pool that holds it: `(pool, [(message id,
    /// attempts)])`, head first.
    pub fn group_buffers(&self, group: &str) -> Vec<(String, Vec<(String, u32)>)> {
        self.all_pools()
            .iter()
            .filter_map(|p| p.group_buffer(group).map(|b| (p.code().to_string(), b)))
            .collect()
    }

    /// Restart every parked group (buffered messages, no drainer) in every
    /// active pool — the backstop sweep the lifecycle reaper runs. Returns
    /// how many groups it restarted.
    pub fn resume_parked_groups(&self) -> usize {
        self.pools
            .iter()
            .map(|e| e.value().clone())
            .collect::<Vec<_>>()
            .iter()
            .map(|p| p.resume_parked_groups())
            .sum()
    }

    /// Every live message group across every pool this manager is
    /// tracking — the operator "blocked groups" view (ledger R-04).
    pub fn blocked_groups(&self) -> Vec<GroupInfo> {
        self.all_pools()
            .iter()
            .flat_map(|p| p.group_snapshot())
            .collect()
    }

    /// One pool's group-flush suppression snapshot for
    /// [`QueueManager::group_flush_snapshots`]: every group currently
    /// suppressed on it, plus its lifetime flush/suppressed counters.
    /// Reshaped into the wire DTO by the API layer.
    pub fn group_flush_snapshots(&self) -> Vec<GroupFlushSnapshot> {
        self.all_pools()
            .iter()
            .map(|p| {
                let stats = p.group_flush_registry().stats();
                GroupFlushSnapshot {
                    pool_code: p.code().to_string(),
                    active_count: stats.active,
                    total_flushes: stats.flushes,
                    total_suppressed: stats.suppressed,
                    groups: p.group_flush_registry().active_suppressions(),
                }
            })
            .collect()
    }

    /// Lift an active group-flush suppression early (operator override,
    /// ledger R-52/R-53). Reports whether a currently-active suppression
    /// existed to lift on the named pool — `false` for an unknown pool
    /// code too, so the API layer can 404 either way.
    pub fn clear_group_flush(&self, pool_code: &str, group: &str) -> bool {
        self.all_pools()
            .iter()
            .find(|p| p.code() == pool_code)
            .is_some_and(|p| p.group_flush_registry().clear(group))
    }

    /// The operator force-ACK override behind the dashboard's in-flight
    /// detail view: deletes the broker copy of a tracked message (using
    /// the freshest receipt handle in `in_pipeline`, which may have been
    /// swapped by a redelivery since the entry was first tracked) and
    /// releases the tracker entry so future redeliveries/external
    /// requeues re-enter the pipeline fresh instead of being ACK-dropped
    /// as duplicates of a stuck/phantom owner.
    ///
    /// The broker ack is best-effort — an expired receipt handle or a
    /// deregistered queue still clears the tracker entry, which is the
    /// part that actually unblocks a phantom (mirrors Go's
    /// `Manager.ForceAckInFlight`). Does NOT abort a worker that is
    /// currently mediating the message — that delivery runs to its own
    /// terminal state; its eventual ack/nack against the now-stale
    /// receipt handle is logged as a warning by the normal callback path,
    /// same as Go's doc says. Returns `None` if the message isn't
    /// currently tracked.
    pub async fn force_ack_in_flight(&self, message_id: &str) -> Option<ForceAckResult> {
        let pipeline_key = self
            .app_message_to_pipeline_key
            .get(message_id)
            .map(|e| e.value().clone())?;
        let entry = self
            .in_pipeline
            .get(&pipeline_key)
            .map(|e| e.value().clone())?;

        // G10: resolve by the consumer's own identifier(), not the config
        // queue name `consumers` is keyed by — `entry.queue_identifier` is
        // `Consumer::identifier()` (see `InFlightMessage::new`), which for
        // NATS differs from the operator-chosen queue name.
        let consumer = self.consumers.resolve(&entry.queue_identifier, 0);
        let (broker_acked, broker_ack_error) = match consumer {
            Some(c) => match ack_bounded(&*c, &entry.receipt_handle).await {
                Ok(()) => (true, None),
                Err(e) => (false, Some(e)),
            },
            None => (
                false,
                Some(format!(
                    "no consumer for queue {:?}",
                    entry.queue_identifier
                )),
            ),
        };

        self.in_pipeline.remove(&pipeline_key);
        self.app_message_to_pipeline_key.remove(message_id);
        self.flight_recorder.record(
            EventKind::Untracked,
            &EventContext::new(entry.message_id.as_str())
                .pool(entry.pool_code.as_str())
                .group(entry.message_group_id.as_deref())
                .queue(entry.queue_identifier.as_str()),
            Facts::text(format!(
                "force-acked by an operator (broker ack {})",
                if broker_acked { "succeeded" } else { "failed" }
            )),
        );

        warn!(
            message_id = %entry.message_id,
            queue = %entry.queue_identifier,
            elapsed_s = entry.elapsed_seconds(),
            broker_acked,
            broker_ack_error = ?broker_ack_error,
            "force-acked in-flight message (operator request)"
        );

        Some(ForceAckResult {
            message_id: entry.message_id.clone(),
            queue_id: entry.queue_identifier.clone(),
            pool_code: entry.pool_code.clone(),
            elapsed_ms: entry.started_at.elapsed().as_millis() as u64,
            broker_acked,
            broker_ack_error,
        })
    }

    /// Get in-flight messages (currently being processed)
    /// Returns messages sorted by elapsed time (oldest first)
    /// Cheap presence check for a single application message ID. O(1).
    pub fn is_in_flight_by_app_id(&self, app_message_id: &str) -> bool {
        match self.app_message_to_pipeline_key.get(app_message_id) {
            Some(e) => self.in_pipeline.contains_key(e.value().as_str()),
            None => false,
        }
    }

    /// Look up a single application message ID in the in-pipeline map.
    ///
    /// Designed for external recovery systems that have a backlog of
    /// messages they suspect are stuck and want to check whether the router
    /// already owns each one before re-enqueueing it. Returns `None` if the
    /// router does not currently hold the message (safe to resend), or a
    /// populated `InFlightMessageInfo` if it does (caller should wait or
    /// skip).
    ///
    /// O(1): goes through `app_message_to_pipeline_key` then `in_pipeline`.
    /// Both are `DashMap`, no global lock.
    pub fn lookup_in_flight_by_app_id(&self, app_message_id: &str) -> Option<InFlightMessageInfo> {
        let pipeline_key = self
            .app_message_to_pipeline_key
            .get(app_message_id)
            .map(|e| e.value().clone())?;
        self.in_pipeline
            .get(&pipeline_key)
            .map(|entry| InFlightMessageInfo::of(entry.value()))
    }

    pub fn get_in_flight_messages(
        &self,
        limit: usize,
        message_id_filter: Option<&str>,
        pool_code_filter: Option<&str>,
    ) -> Vec<InFlightMessageInfo> {
        let mut messages: Vec<InFlightMessageInfo> = self
            .in_pipeline
            .iter()
            .filter(|entry| {
                let msg = entry.value();
                // Message ID filter: substring match, case-insensitive
                if let Some(filter) = message_id_filter {
                    if !msg
                        .message_id
                        .to_lowercase()
                        .contains(&filter.to_lowercase())
                    {
                        return false;
                    }
                }
                // Pool code filter: exact match, case-insensitive
                if let Some(filter) = pool_code_filter {
                    if !msg.pool_code.eq_ignore_ascii_case(filter) {
                        return false;
                    }
                }
                true
            })
            .map(|entry| InFlightMessageInfo::of(entry.value()))
            .collect();

        // Sort by elapsed time descending (oldest first)
        messages.sort_by_key(|m| Reverse(m.elapsed_time_ms));

        // Apply limit
        messages.truncate(limit);
        messages
    }

    /// Get count of in-flight messages
    pub fn in_flight_count(&self) -> usize {
        self.in_pipeline.len()
    }
}

/// Information about an in-flight message for API response
#[derive(Debug, Clone, serde::Serialize, ToSchema)]
pub struct InFlightMessageInfo {
    #[serde(rename = "messageId")]
    pub message_id: String,
    #[serde(rename = "brokerMessageId")]
    pub broker_message_id: Option<String>,
    #[serde(rename = "queueId")]
    pub queue_id: String,
    #[serde(rename = "poolCode")]
    pub pool_code: String,
    #[serde(rename = "elapsedTimeMs")]
    pub elapsed_time_ms: u64,
    #[serde(rename = "addedToInPipelineAt")]
    pub added_to_in_pipeline_at: chrono::DateTime<chrono::Utc>,
    /// FIFO message-group id, empty string for ungrouped. Additive field,
    /// matches Go's `InFlightMessageInfo.MessageGroup`.
    #[serde(rename = "messageGroup")]
    pub message_group: String,
    /// In-place retry attempts the pool has recorded for this admission
    /// (Go's `InFlightTracker.MarkRetrying`, `InFlightMessageInfo.Attempts`).
    pub attempts: u32,
    /// When the broker last (re)delivered it — refreshed on every
    /// redelivery; the reaper's idle clock (Go `LastSeenAt`, the "phantom
    /// entry" signal: an entry whose last-seen keeps ageing is no longer
    /// being redelivered).
    #[serde(rename = "lastSeenAt")]
    pub last_seen_at: chrono::DateTime<chrono::Utc>,
    #[serde(rename = "lastSeenElapsedMs")]
    pub last_seen_elapsed_ms: u64,
    /// A live in-place retry (an attempt recorded within the reaper's
    /// grace): the detail view's `RETRY_BACKOFF`.
    #[serde(skip)]
    pub retrying: bool,
}

impl InFlightMessageInfo {
    fn of(t: &super::tracking::Tracked) -> Self {
        let now = Instant::now();
        let wall = chrono::Utc::now();
        let ago = |i: Instant| {
            wall - chrono::Duration::milliseconds(
                now.saturating_duration_since(i).as_millis() as i64
            )
        };
        Self {
            message_id: t.message_id.clone(),
            broker_message_id: t.broker_message_id.clone(),
            queue_id: t.queue_identifier.clone(),
            pool_code: t.pool_code.clone(),
            elapsed_time_ms: now.saturating_duration_since(t.started_at).as_millis() as u64,
            added_to_in_pipeline_at: ago(t.started_at),
            message_group: t.message_group_id.clone().unwrap_or_default(),
            attempts: t.attempts,
            last_seen_at: ago(t.last_seen),
            last_seen_elapsed_ms: now.saturating_duration_since(t.last_seen).as_millis() as u64,
            retrying: t.is_retrying(now),
        }
    }
}

/// Reports what [`QueueManager::force_ack_in_flight`] did: the entry as it
/// stood when acked, and the outcome of the best-effort broker delete.
/// Mirrors Go's `ForceAckResult` — reshaped into the wire `ForceAckResponse`
/// DTO by the API layer, same as Go's `handlers_mutations.go` does.
#[derive(Debug, Clone)]
pub struct ForceAckResult {
    pub message_id: String,
    pub queue_id: String,
    pub pool_code: String,
    pub elapsed_ms: u64,
    pub broker_acked: bool,
    pub broker_ack_error: Option<String>,
}

/// One pool's group-flush suppression snapshot, as
/// [`QueueManager::group_flush_snapshots`] reports it — reshaped into the
/// wire `GroupFlushPoolInfo` DTO by the API layer. Mirrors Go's
/// (unexported) `GroupFlushSnapshot`.
#[derive(Debug, Clone)]
pub struct GroupFlushSnapshot {
    pub pool_code: String,
    pub active_count: usize,
    pub total_flushes: u64,
    pub total_suppressed: u64,
    pub groups: Vec<GroupSuppression>,
}
