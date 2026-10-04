//! Renders and publishes a claim.
//!
//! A port of Go's `MessageGroupDispatcher` (`scheduler/dispatcher.go`): the
//! whole claim goes to the publisher in one call, in claim order, and the
//! publisher reports exactly which ids it did not publish. Ordering comes
//! from the claim order plus the FIFO queue, not from in-process
//! serialisation. Unlike Go, the poller publishes while its claim
//! transaction is still open and marks only the published ids QUEUED (see
//! [`super::poller`]); [`revert_unpublished`] remains for a claim that was
//! committed QUEUED first.

use std::sync::Arc;
use std::time::Duration;

use fc_common::{MediationType, Message};
use sqlx::PgPool;
use tokio::time;
use tracing::warn;

use super::auth::DispatchAuthService;
use super::publisher::{DispatchPublisher, PublishItem, PublishOutcome};
use crate::dispatch_job::entity;
use tracing::field::Empty;

/// The longest one claim's publish may take, end to end. The poller holds a
/// pool connection and up to a batch of row locks for as long as the publish
/// runs, so a hung broker call must be cut off: past this the claim counts
/// as not published and its transaction rolls back (the rows stay PENDING).
/// Well above the SQS client's own operation timeout times the chunk count
/// of a normal claim.
pub const PUBLISH_CLAIM_TIMEOUT: Duration = Duration::from_secs(120);

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
    publisher: Arc<dyn DispatchPublisher>,
    auth: DispatchAuthService,
    processing_endpoint: String,
    publish_timeout: Duration,
}

impl MessageGroupDispatcher {
    pub fn new(
        publisher: Arc<dyn DispatchPublisher>,
        auth: DispatchAuthService,
        processing_endpoint: String,
    ) -> Self {
        Self {
            publisher,
            auth,
            processing_endpoint,
            publish_timeout: PUBLISH_CLAIM_TIMEOUT,
        }
    }

    /// Override [`PUBLISH_CLAIM_TIMEOUT`] (tests).
    #[cfg(test)]
    fn with_publish_timeout(mut self, timeout: Duration) -> Self {
        self.publish_timeout = timeout;
        self
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
            dispatch_mode: entity::parse_dispatch_mode(Some(&tok.mode)),
            dispatch_mode_specified: true,
        }
    }

    /// Publish a claim, in claim order, and report what did not publish.
    /// The caller marks only the rest QUEUED.
    #[tracing::instrument(
        name = "scheduler.publish",
        skip_all,
        fields(jobs = tokens.len(), unpublished = Empty)
    )]
    pub async fn publish_claim(&self, tokens: &[DispatchJobToken]) -> PublishOutcome {
        if tokens.is_empty() {
            return PublishOutcome::default();
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
        // A publish that outlives the deadline is dropped and reported as
        // nothing published: the caller then rolls the claim back, so a
        // timeout can never mark a job QUEUED. Anything the broker did take
        // is re-published next poll (at-least-once; `/process` delivers once).
        let started = time::Instant::now();
        let outcome = match time::timeout(self.publish_timeout, self.publisher.publish(items)).await
        {
            Ok(outcome) => outcome,
            Err(_) => {
                metrics::counter!("scheduler.publish.timeouts_total").increment(1);
                PublishOutcome {
                    unpublished: tokens.iter().map(|t| t.job_id.clone()).collect(),
                    error: Some(format!(
                        "publish did not finish within {}s",
                        self.publish_timeout.as_secs()
                    )),
                }
            }
        };
        metrics::histogram!("scheduler.publish.duration_seconds").record(started.elapsed());
        tracing::Span::current().record("unpublished", outcome.unpublished.len());
        if let Some(e) = &outcome.error {
            warn!(unpublished = outcome.unpublished.len(), of = total, error = %e,
                "dispatch publish failed");
        }
        metrics::counter!("scheduler.jobs.queued_total")
            .increment((total - outcome.unpublished.len()) as u64);
        if !outcome.unpublished.is_empty() {
            metrics::counter!("scheduler.jobs.dispatch_errors_total")
                .increment(outcome.unpublished.len() as u64);
        }
        outcome
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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::future::pending;

    /// A publisher whose broker call never returns.
    struct Hung;

    #[async_trait]
    impl DispatchPublisher for Hung {
        async fn publish(&self, _items: Vec<PublishItem>) -> PublishOutcome {
            pending().await
        }
        fn describe(&self) -> String {
            "hung".into()
        }
    }

    fn token(id: &str) -> DispatchJobToken {
        DispatchJobToken {
            job_id: id.into(),
            message_group: None,
            mode: "IMMEDIATE".into(),
            pool_code: "P".into(),
            client_id: None,
            subscription_id: None,
            queue: None,
        }
    }

    /// A hung publish is cut off at the deadline and reports every job
    /// unpublished, so the poller rolls back and marks nothing QUEUED.
    #[tokio::test(start_paused = true)]
    async fn a_hung_publish_times_out_and_publishes_nothing() {
        let d = MessageGroupDispatcher::new(
            Arc::new(Hung),
            DispatchAuthService::with_secret("s"),
            "http://x/process".into(),
        );
        let started = time::Instant::now();
        let out = d.publish_claim(&[token("a"), token("b"), token("c")]).await;
        assert_eq!(out.unpublished, vec!["a", "b", "c"]);
        assert!(out.error.unwrap().contains("did not finish"));
        assert_eq!(started.elapsed(), PUBLISH_CLAIM_TIMEOUT);
    }

    /// The deadline does not touch a publish that finishes in time.
    #[tokio::test(start_paused = true)]
    async fn a_prompt_publish_is_untouched() {
        struct Fast;
        #[async_trait]
        impl DispatchPublisher for Fast {
            async fn publish(&self, _items: Vec<PublishItem>) -> PublishOutcome {
                PublishOutcome::default()
            }
            fn describe(&self) -> String {
                "fast".into()
            }
        }
        let d = MessageGroupDispatcher::new(
            Arc::new(Fast),
            DispatchAuthService::with_secret("s"),
            "http://x/process".into(),
        )
        .with_publish_timeout(Duration::from_secs(1));
        let out = d.publish_claim(&[token("a")]).await;
        assert!(out.unpublished.is_empty());
    }
}
