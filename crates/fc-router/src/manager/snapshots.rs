//! Read-side views: pool stats/capacity, the operator dashboard snapshots
//! (mediating entries, blocked groups, group-flush suppressions), the
//! force-ack override, and in-flight message lookups.

use std::sync::Arc;

use tracing::warn;
use utoipa::ToSchema;

use fc_common::PoolStats;

use crate::pool::ProcessPool;

use super::{PoolState, QueueManager};

impl QueueManager {
    /// Check if any pool has capacity to accept messages.
    /// Used to gate SQS polling — avoids a hot poll-defer loop when all pools are full.
    /// Only considers `Active` entries — a draining pool is never a routing
    /// candidate (see the `pools` field's doc comment).
    pub(super) fn has_pool_capacity(&self) -> bool {
        let mut saw_active = false;
        for entry in self.pools.iter() {
            if entry.value().state != PoolState::Active {
                continue;
            }
            saw_active = true;
            if entry.value().pool.available_capacity() > 0 {
                return true;
            }
        }
        !saw_active
    }

    /// Get statistics for all active pools.
    pub fn get_pool_stats(&self) -> Vec<PoolStats> {
        self.pools
            .iter()
            .filter(|e| e.value().state == PoolState::Active)
            .map(|entry| entry.value().pool.get_stats())
            .collect()
    }

    /// Every pool this manager is tracking — active, still draining after
    /// removal, AND any draining predecessor displaced from `pools` by a
    /// later Active insert under the same code (`orphaned_draining` — see
    /// that field's doc comment). Mirrors Go's `Manager.AllPools`: a pool
    /// still finishing an asynchronous removal-drain (X-11) can still be
    /// holding buffered groups, live group-flush suppressions, or
    /// in-worker deliveries worth showing on the operator dashboard, so the
    /// "Mediating"/"blocked groups"/"group flushes" views traverse every
    /// entry regardless of state — including an orphan, so a displaced
    /// predecessor's still-buffered/in-flight work never silently drops off
    /// the dashboard.
    fn all_pools(&self) -> Vec<Arc<ProcessPool>> {
        let mut all: Vec<Arc<ProcessPool>> =
            self.pools.iter().map(|e| e.value().pool.clone()).collect();
        all.extend(self.orphaned_draining.lock().iter().cloned());
        all
    }

    /// Every message currently inside a worker, across every pool this
    /// manager is tracking — the operator "Mediating" dashboard view.
    pub fn mediating_snapshot(&self) -> Vec<crate::pool::MediatingEntry> {
        self.all_pools()
            .iter()
            .flat_map(|p| p.mediating_snapshot())
            .collect()
    }

    /// Every live message group across every pool this manager is
    /// tracking — the operator "blocked groups" view (ledger R-04).
    pub fn blocked_groups(&self) -> Vec<crate::pool::GroupInfo> {
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
        let entry = self.in_pipeline.get(&pipeline_key).map(|e| e.value().clone())?;

        let consumer = self
            .consumers
            .read()
            .await
            .get(&entry.queue_identifier)
            .cloned();
        let (broker_acked, broker_ack_error) = match consumer {
            Some(c) => match c.ack(&entry.receipt_handle).await {
                Ok(()) => (true, None),
                Err(e) => (false, Some(e.to_string())),
            },
            None => (
                false,
                Some(format!("no consumer for queue {:?}", entry.queue_identifier)),
            ),
        };

        self.in_pipeline.remove(&pipeline_key);
        self.app_message_to_pipeline_key.remove(message_id);

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
        self.in_pipeline.get(&pipeline_key).map(|entry| {
            let msg = entry.value();
            let elapsed = msg.started_at.elapsed();
            InFlightMessageInfo {
                message_id: msg.message_id.clone(),
                broker_message_id: msg.broker_message_id.clone(),
                queue_id: msg.queue_identifier.clone(),
                pool_code: msg.pool_code.clone(),
                elapsed_time_ms: elapsed.as_millis() as u64,
                added_to_in_pipeline_at: chrono::Utc::now()
                    - chrono::Duration::milliseconds(elapsed.as_millis() as i64),
                message_group: msg.message_group_id.clone().unwrap_or_default(),
                attempts: 0,
            }
        })
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
                // Message ID filter: substring match, case-insensitive (matches Java)
                if let Some(filter) = message_id_filter {
                    if !msg
                        .message_id
                        .to_lowercase()
                        .contains(&filter.to_lowercase())
                    {
                        return false;
                    }
                }
                // Pool code filter: exact match, case-insensitive (matches Java)
                if let Some(filter) = pool_code_filter {
                    if !msg.pool_code.eq_ignore_ascii_case(filter) {
                        return false;
                    }
                }
                true
            })
            .map(|entry| {
                let msg = entry.value();
                InFlightMessageInfo {
                    message_id: msg.message_id.clone(),
                    broker_message_id: msg.broker_message_id.clone(),
                    queue_id: msg.queue_identifier.clone(),
                    pool_code: msg.pool_code.clone(),
                    elapsed_time_ms: msg.started_at.elapsed().as_millis() as u64,
                    added_to_in_pipeline_at: chrono::Utc::now()
                        - chrono::Duration::milliseconds(
                            msg.started_at.elapsed().as_millis() as i64
                        ),
                    message_group: msg.message_group_id.clone().unwrap_or_default(),
                    attempts: 0,
                }
            })
            .collect();

        // Sort by elapsed time descending (oldest first)
        messages.sort_by_key(|m| std::cmp::Reverse(m.elapsed_time_ms));

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
    /// In-pipeline retry count. **Always 0 in this port** — see
    /// `MediatingEntry::attempts`'s doc for why: this Rust port's pool has
    /// no in-pipeline retry-with-front-reinsertion concept at all, unlike
    /// Go's `InFlightTracker.MarkRetrying`. Additive field, matches Go's
    /// `InFlightMessageInfo.Attempts`.
    pub attempts: u32,
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
    pub groups: Vec<crate::group_flush::GroupSuppression>,
}
