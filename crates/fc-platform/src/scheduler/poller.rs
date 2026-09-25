//! Pending job poller.
//!
//! A port of Go's `PendingJobPoller` (`scheduler/poller.go`). One tick:
//!
//! 1. In one transaction, claim up to `batch_size` PENDING jobs that are due
//!    (`scheduled_for` NULL or past) with `FOR UPDATE SKIP LOCKED`, in the
//!    total order `(message_group NULLS LAST, sequence, created_at, id)`.
//!    Concurrent schedulers therefore never claim the same row.
//! 2. With the rows still locked, publish the claim in one call (see
//!    [`MessageGroupDispatcher`]), mark exactly the published ids QUEUED, and
//!    commit.
//!
//! **Deliberately not Go's order.** Go commits the claim QUEUED first and
//! publishes after; a worker that dies between the two (a SIGKILL, an OOM, a
//! deploy past its stop timeout) leaves the unpublished rows QUEUED with no
//! queue message, and nothing looks at them again until stale recovery's
//! 75 minutes are up (delivery run 3, `worker-restart`: Go stranded the last
//! two jobs of its publish). Here the claim only commits once the publish is
//! done, so a worker that dies mid-publish rolls the whole claim back to
//! PENDING and the next poll (its own after a restart, or a standby's)
//! publishes it again. The price is at-least-once at the queue: the jobs it
//! did publish before dying are published a second time. That costs no
//! second delivery — `/api/dispatch/process` claims a job before delivering
//! it, and a copy that finds the job taken or finished never delivers it —
//! only a second, redundant queue message. A `/process` call that arrives
//! for a job before this commit waits on the row lock for it.
//!
//! Held and paused jobs are excluded **inside the claim query**, not after
//! it (a deliberate improvement on Go, which filters after the `LIMIT`: a
//! full batch of held or paused rows at the head of the order would stall
//! every other group). Excluded:
//! - jobs whose subscription's connection is PAUSED (cached set, refreshed
//!   every `paused_cache_ttl`);
//! - BLOCK_ON_ERROR jobs with an EARLIER job in their group that is holding
//!   it: FAILED/ERROR, or PENDING in a retry backoff
//!   ([`GROUP_HOLDING_STATUS_SQL`]). The comparison is positional, so the
//!   holder itself dispatches once its backoff expires. IMMEDIATE and
//!   NEXT_ON_ERROR (and, per X-01, an absent or unknown mode) never wait.
//!
//! A NULL group never holds (`=` never matches NULL); an ungrouped job is
//! held only by a row whose group is literally `default` — Go's quirk,
//! kept.

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use sqlx::PgPool;
use tracing::{debug, trace, warn};

use super::destination::PoolCodeResolver;
use super::dispatcher::{DispatchJobToken, MessageGroupDispatcher};
use super::SchedulerError;

/// A job that is holding its message group (Go `GroupHoldingStatusSQL`):
/// terminally failed, or waiting out a retry backoff. QUEUED and PROCESSING
/// are the normal flow and hold nothing. Unqualified column names; callers
/// qualify by aliasing the table.
pub const GROUP_HOLDING_STATUS_SQL: &str =
    "status IN ('FAILED', 'ERROR') OR (status = 'PENDING' AND scheduled_for IS NOT NULL AND scheduled_for > NOW())";

/// The claim query: due PENDING jobs, minus paused subscriptions and held
/// BLOCK_ON_ERROR successors, locked and totally ordered.
const CLAIM_SQL: &str = "\
SELECT j.id, j.subscription_id, j.message_group, j.mode, j.dispatch_pool_id, j.client_id, \
       j.created_at, j.queue \
  FROM msg_dispatch_jobs j \
 WHERE j.status = 'PENDING' \
   AND (j.scheduled_for IS NULL OR j.scheduled_for <= NOW()) \
   AND (j.subscription_id IS NULL OR NOT (j.subscription_id = ANY($2::text[]))) \
   AND NOT (j.mode = 'BLOCK_ON_ERROR' AND EXISTS ( \
        SELECT 1 FROM msg_dispatch_jobs h \
         WHERE h.message_group = COALESCE(j.message_group, 'default') \
           AND (h.status IN ('FAILED', 'ERROR') \
                OR (h.status = 'PENDING' AND h.scheduled_for IS NOT NULL AND h.scheduled_for > NOW())) \
           AND (h.sequence, h.created_at, h.id) < (j.sequence, j.created_at, j.id))) \
 ORDER BY j.message_group ASC NULLS LAST, j.sequence ASC, j.created_at ASC, j.id ASC \
 LIMIT $1 \
 FOR UPDATE OF j SKIP LOCKED";

#[derive(Debug, sqlx::FromRow)]
struct ClaimRow {
    id: String,
    subscription_id: Option<String>,
    message_group: Option<String>,
    mode: String,
    dispatch_pool_id: Option<String>,
    client_id: Option<String>,
    created_at: DateTime<Utc>,
    queue: Option<String>,
}

/// The subscription ids whose connection is PAUSED, cached.
pub struct PausedConnectionCache {
    pool: PgPool,
    ttl: Duration,
    state: RwLock<(Vec<String>, Option<Instant>)>,
}

impl PausedConnectionCache {
    pub fn new(pool: PgPool, ttl: Duration) -> Self {
        Self {
            pool,
            ttl,
            state: RwLock::new((Vec::new(), None)),
        }
    }

    /// The cached set, refreshed when stale. A refresh failure fails the
    /// tick (Go): claiming without knowing what is paused would dispatch
    /// paused work.
    pub async fn paused_subscription_ids(&self) -> Result<Vec<String>, SchedulerError> {
        {
            let s = self.state.read();
            if s.1.is_some_and(|t| t.elapsed() < self.ttl) {
                return Ok(s.0.clone());
            }
        }
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT s.id FROM msg_subscriptions s \
             JOIN msg_connections c ON c.id = s.connection_id \
             WHERE c.status = 'PAUSED'",
        )
        .fetch_all(&self.pool)
        .await?;
        debug!(
            paused_subscriptions = ids.len(),
            "paused connection cache refreshed"
        );
        *self.state.write() = (ids.clone(), Some(Instant::now()));
        Ok(ids)
    }
}

pub struct PendingJobPoller {
    pool: PgPool,
    batch_size: usize,
    dispatcher: Arc<MessageGroupDispatcher>,
    paused: PausedConnectionCache,
    pool_codes: Arc<PoolCodeResolver>,
}

/// What one tick did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PollReport {
    pub claimed: usize,
    pub published: usize,
}

impl PendingJobPoller {
    pub fn new(
        pool: PgPool,
        batch_size: usize,
        paused_cache_ttl: Duration,
        dispatcher: Arc<MessageGroupDispatcher>,
        pool_codes: Arc<PoolCodeResolver>,
    ) -> Self {
        Self {
            paused: PausedConnectionCache::new(pool.clone(), paused_cache_ttl),
            pool,
            batch_size,
            dispatcher,
            pool_codes,
        }
    }

    /// Claim, mark QUEUED, commit, publish, revert the unpublished.
    pub async fn poll_once(&self) -> Result<PollReport, SchedulerError> {
        let paused = self.paused.paused_subscription_ids().await?;

        let mut tx = self.pool.begin().await?;
        let claimed: Vec<ClaimRow> = sqlx::query_as(CLAIM_SQL)
            .bind(self.batch_size as i64)
            .bind(&paused)
            .fetch_all(&mut *tx)
            .await?;
        if claimed.is_empty() {
            tx.rollback().await.ok();
            trace!("no pending jobs");
            return Ok(PollReport::default());
        }

        let created: std::collections::HashMap<String, DateTime<Utc>> = claimed
            .iter()
            .map(|c| (c.id.clone(), c.created_at))
            .collect();
        let mut tokens = Vec::with_capacity(claimed.len());
        for c in claimed {
            let pool_code = self
                .pool_codes
                .resolve(c.dispatch_pool_id.as_deref(), c.client_id.as_deref())
                .await;
            tokens.push(DispatchJobToken {
                job_id: c.id,
                message_group: c.message_group,
                mode: c.mode,
                pool_code,
                client_id: c.client_id,
                subscription_id: c.subscription_id,
                queue: c.queue,
            });
        }
        let claimed = tokens.len();
        metrics::gauge!("scheduler.pending_jobs").set(claimed as f64);

        // Publish while the claim is still locked and uncommitted: see the
        // module doc. What did not publish simply stays PENDING.
        let outcome = self.dispatcher.publish_claim(&tokens).await;
        let unpublished: std::collections::HashSet<&str> =
            outcome.unpublished.iter().map(String::as_str).collect();
        let (ids, created_ats): (Vec<&str>, Vec<DateTime<Utc>>) = tokens
            .iter()
            .map(|t| t.job_id.as_str())
            .filter(|id| !unpublished.contains(id))
            .map(|id| (id, created[id]))
            .unzip();
        let published = ids.len();
        if published == 0 {
            tx.rollback().await.ok();
            debug!(claimed, published, "poll tick");
            return Ok(PollReport { claimed, published });
        }
        let marked = async {
            sqlx::query(
                "UPDATE msg_dispatch_jobs SET status = 'QUEUED', queued_at = NOW(), updated_at = NOW() \
                 FROM UNNEST($1::varchar[], $2::timestamptz[]) AS t(id, created_at) \
                 WHERE msg_dispatch_jobs.id = t.id AND msg_dispatch_jobs.created_at = t.created_at",
            )
            .bind(&ids)
            .bind(&created_ats)
            .execute(&mut *tx)
            .await?;
            tx.commit().await
        }
        .await;
        if let Err(e) = marked {
            // Published but still PENDING: the next poll publishes them
            // again, and `/process` delivers each once.
            warn!(published, error = %e,
                "marking published dispatch jobs QUEUED failed; they will be published again");
            return Err(e.into());
        }
        debug!(claimed, published, "poll tick");
        Ok(PollReport { claimed, published })
    }

    /// Drive the poller every `interval` until cancelled, only while
    /// `is_leader` says so (the per-group order needs one active scheduler).
    pub async fn run(
        &self,
        interval: Duration,
        is_leader: Arc<dyn Fn() -> bool + Send + Sync>,
        cancel: tokio_util::sync::CancellationToken,
    ) {
        let mut tick = tokio::time::interval(interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tick.tick() => {}
            }
            if !is_leader() {
                continue;
            }
            if let Err(e) = self.poll_once().await {
                warn!(error = %e, "dispatch poll error");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_sql_locks_only_the_job_rows_and_orders_totally() {
        assert!(CLAIM_SQL.contains("FOR UPDATE OF j SKIP LOCKED"));
        assert!(CLAIM_SQL.contains(
            "ORDER BY j.message_group ASC NULLS LAST, j.sequence ASC, j.created_at ASC, j.id ASC"
        ));
        assert!(CLAIM_SQL.contains("j.scheduled_for IS NULL OR j.scheduled_for <= NOW()"));
    }

    /// The claim inlines the holding condition under an alias; it must be
    /// the condition /process checks at delivery time.
    #[test]
    fn the_claim_hold_matches_the_shared_definition() {
        let normalise = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
        let claim = normalise(CLAIM_SQL);
        let aliased = GROUP_HOLDING_STATUS_SQL
            .replace("status", "h.status")
            .replace("scheduled_for", "h.scheduled_for");
        let (failed, backoff) = aliased.split_once(" OR ").unwrap();
        assert!(claim.contains(failed), "{claim}");
        assert!(claim.contains(backoff), "{claim}");
    }
}
