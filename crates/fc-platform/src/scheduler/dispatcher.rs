//! Publishes a committed claim and reverts what did not publish.
//!
//! A port of Go's `MessageGroupDispatcher` (`scheduler/dispatcher.go`): the
//! whole claim goes to the publisher in one call, in claim order, and only
//! the ids the publisher reports unpublished go back to PENDING — guarded on
//! `status = 'QUEUED'` so a job `/api/dispatch/process` has already advanced
//! is left alone. Ordering comes from the claim order plus the FIFO queue,
//! not from in-process serialisation.

use std::sync::Arc;

use fc_common::{MediationType, Message};
use sqlx::PgPool;
use tracing::warn;

use super::auth::DispatchAuthService;
use super::publisher::{DispatchPublisher, PublishItem};

/// What the poller hands the dispatcher for one claimed job.
#[derive(Debug, Clone)]
pub struct DispatchJobToken {
    pub job_id: String,
    pub message_group: Option<String>,
    /// The raw stored mode; parsed per X-01 when the message is built.
    pub mode: String,
    /// The resolved, client-namespaced pool code.
    pub pool_code: String,
    pub client_id: Option<String>,
    pub subscription_id: Option<String>,
    /// The job's own priority claim, raw.
    pub queue: Option<String>,
}

pub struct MessageGroupDispatcher {
    pool: PgPool,
    publisher: Arc<dyn DispatchPublisher>,
    auth: DispatchAuthService,
    processing_endpoint: String,
}

impl MessageGroupDispatcher {
    pub fn new(
        pool: PgPool,
        publisher: Arc<dyn DispatchPublisher>,
        auth: DispatchAuthService,
        processing_endpoint: String,
    ) -> Self {
        Self {
            pool,
            publisher,
            auth,
            processing_endpoint,
        }
    }

    /// The queue message for a claimed job. `mediation_target` is the
    /// platform's processing endpoint, never the subscriber's URL; the
    /// signed token lets that endpoint verify the callback.
    pub fn build_message(&self, tok: &DispatchJobToken) -> Message {
        Message {
            id: tok.job_id.clone(),
            pool_code: tok.pool_code.clone(),
            auth_token: Some(self.auth.sign(&tok.job_id)),
            signing_secret: None,
            mediation_type: MediationType::HTTP,
            mediation_target: self.processing_endpoint.clone(),
            message_group_id: tok.message_group.clone().filter(|g| !g.is_empty()),
            high_priority: false,
            dispatch_mode: crate::dispatch_job::entity::parse_dispatch_mode(Some(&tok.mode)),
            dispatch_mode_specified: true,
        }
    }

    /// Publish the claim; revert exactly the unpublished ids. Returns how
    /// many were published.
    pub async fn submit_batch(&self, tokens: Vec<DispatchJobToken>) -> usize {
        if tokens.is_empty() {
            return 0;
        }
        let total = tokens.len();
        let items: Vec<PublishItem> = tokens
            .iter()
            .map(|t| PublishItem {
                job_id: t.job_id.clone(),
                client_id: t.client_id.clone(),
                subscription_id: t.subscription_id.clone(),
                queue: t.queue.clone(),
                message: self.build_message(t),
            })
            .collect();
        let outcome = self.publisher.publish(items).await;
        if let Some(e) = &outcome.error {
            warn!(unpublished = outcome.unpublished.len(), of = total, error = %e,
                "dispatch publish failed");
        }
        metrics::counter!("scheduler.jobs.queued_total")
            .increment((total - outcome.unpublished.len()) as u64);
        if outcome.unpublished.is_empty() {
            return total;
        }
        metrics::counter!("scheduler.jobs.dispatch_errors_total")
            .increment(outcome.unpublished.len() as u64);
        let published = total - outcome.unpublished.len();
        if let Err(e) = revert_unpublished(&self.pool, &outcome.unpublished).await {
            warn!(count = outcome.unpublished.len(), error = %e,
                "revert of unpublished jobs failed; stale recovery will reclaim them");
        }
        published
    }
}

/// `QUEUED → PENDING` for jobs that never reached the broker. Only rows
/// still QUEUED: anything `/process` advanced is left alone.
pub async fn revert_unpublished(pool: &PgPool, ids: &[String]) -> Result<u64, sqlx::Error> {
    let r = sqlx::query(
        "UPDATE msg_dispatch_jobs SET status = 'PENDING', queued_at = NULL, updated_at = NOW() \
         WHERE id = ANY($1) AND status = 'QUEUED'",
    )
    .bind(ids)
    .execute(pool)
    .await?;
    Ok(r.rows_affected())
}
