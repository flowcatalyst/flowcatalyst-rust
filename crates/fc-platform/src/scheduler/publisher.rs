//! Hands claimed dispatch jobs to the queues the router consumes.
//!
//! A port of Go's `DispatchPublisher` contract
//! (`internal/platform/scheduler/publisher.go`): the poller calls
//! [`DispatchPublisher::publish`] once per tick, after the claim transaction
//! has committed, and reverts exactly the ids it reports unpublished. A
//! publisher must never report an id the broker accepted as unpublished (it
//! would be delivered twice), nor the reverse (it would strand at QUEUED).
//!
//! Three publishers:
//! - [`SqsDispatchPublisher`]: per-(tenant, priority) SQS FIFO queues,
//!   created lazily (production; Go's `SQSDispatchPublisher`).
//! - [`PostgresDispatchPublisher`]: per-(tenant, priority) rows in the
//!   platform database's `queue_messages` (Go's `PostgresDispatchPublisher`).
//! - [`SingleQueuePublisher`]: one fixed local queue (fc-dev, whose embedded
//!   router consumes exactly one queue).

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use fc_common::{DispatchMode, MediationType, Message};
use fc_queue::sqs_publisher::{
    AwsSqsBatchApi, FifoPublishItem, QueueAddressing, SqsBatchApi, SqsFifoPublisher,
};
use fc_queue::{EmbeddedQueue, QueuePublisher};
use serde::Serialize;
use sqlx::PgPool;
use tracing::{debug, warn};

use super::destination::{DestinationInput, DestinationResolver};

/// One claimed job on its way to a queue: the rendered message plus the ids
/// its destination is resolved from.
#[derive(Debug, Clone)]
pub struct PublishItem {
    pub job_id: String,
    pub client_id: Option<String>,
    pub subscription_id: Option<String>,
    /// The job's own stored priority claim (`msg_dispatch_jobs.queue`), raw.
    pub queue: Option<String>,
    pub message: Message,
}

impl PublishItem {
    fn destination_input(&self) -> DestinationInput<'_> {
        DestinationInput {
            client_id: self.client_id.as_deref(),
            subscription_id: self.subscription_id.as_deref(),
            queue: self.queue.as_deref(),
        }
    }

    /// The FIFO group: the job's message group, or its own id when it has
    /// none (a singleton group imposes no order).
    pub fn group_id(&self) -> &str {
        match self.message.message_group_id.as_deref() {
            Some(g) if !g.is_empty() => g,
            _ => &self.job_id,
        }
    }
}

/// The ids not published, and the last error for logging.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PublishOutcome {
    pub unpublished: Vec<String>,
    pub error: Option<String>,
}

impl PublishOutcome {
    fn all(items: &[PublishItem], error: String) -> Self {
        Self {
            unpublished: items.iter().map(|i| i.job_id.clone()).collect(),
            error: Some(error),
        }
    }
}

#[async_trait]
pub trait DispatchPublisher: Send + Sync {
    /// Publish a claim-ordered batch, returning the ids NOT published.
    async fn publish(&self, items: Vec<PublishItem>) -> PublishOutcome;

    /// A short description for the startup log.
    fn describe(&self) -> String;
}

/// The queue message body exactly as Go's `json.Marshal(common.Message)`
/// renders it: empty optional fields are omitted, not `null`.
pub fn wire_body(message: &Message) -> String {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Wire<'a> {
        id: &'a str,
        #[serde(skip_serializing_if = "str::is_empty")]
        pool_code: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        auth_token: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        signing_secret: Option<&'a str>,
        mediation_type: MediationType,
        mediation_target: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        message_group_id: Option<&'a str>,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        high_priority: bool,
        dispatch_mode: DispatchMode,
    }
    serde_json::to_string(&Wire {
        id: &message.id,
        pool_code: &message.pool_code,
        auth_token: message.auth_token.as_deref(),
        signing_secret: message.signing_secret.as_deref(),
        mediation_type: message.mediation_type,
        mediation_target: &message.mediation_target,
        message_group_id: message.message_group_id.as_deref(),
        high_priority: message.high_priority,
        dispatch_mode: message.dispatch_mode,
    })
    .expect("a message always serialises")
}

// ── SQS ─────────────────────────────────────────────────────────────────

/// Publishes to per-(tenant, priority) SQS FIFO queues.
pub struct SqsDispatchPublisher<A: SqsBatchApi = AwsSqsBatchApi> {
    inner: SqsFifoPublisher<A>,
    destinations: Arc<DestinationResolver>,
}

impl<A: SqsBatchApi> SqsDispatchPublisher<A> {
    pub fn new(inner: SqsFifoPublisher<A>, destinations: Arc<DestinationResolver>) -> Self {
        Self {
            inner,
            destinations,
        }
    }
}

#[async_trait]
impl<A: SqsBatchApi + 'static> DispatchPublisher for SqsDispatchPublisher<A> {
    async fn publish(&self, items: Vec<PublishItem>) -> PublishOutcome {
        let mut fifo = Vec::with_capacity(items.len());
        let mut unpublished = Vec::new();
        let mut last_error = None;
        for item in &items {
            match self
                .destinations
                .destination(&item.destination_input())
                .await
            {
                Ok(queue_name) => fifo.push(FifoPublishItem {
                    id: item.job_id.clone(),
                    queue_name,
                    group_id: item.group_id().to_string(),
                    body: wire_body(&item.message),
                }),
                Err(e) => {
                    // Costs only this job: it reverts and retries next poll.
                    warn!(job_id = %item.job_id, error = %e,
                        "could not resolve a dispatch destination; job will revert to PENDING");
                    unpublished.push(item.job_id.clone());
                    last_error = Some(e.to_string());
                }
            }
        }
        let out = self.inner.publish(fifo).await;
        unpublished.extend(out.unpublished);
        PublishOutcome {
            unpublished,
            error: out.error.or(last_error),
        }
    }

    fn describe(&self) -> String {
        match self.inner.addressing() {
            QueueAddressing::Composed { region, account_id } => {
                format!("SQS FIFO queues in {region} (account {account_id})")
            }
            QueueAddressing::Base(base) => format!("SQS FIFO queues under {base}"),
        }
    }
}

// ── Postgres ────────────────────────────────────────────────────────────

/// Publishes to per-(tenant, priority) queue names in the platform
/// database's `queue_messages` table.
pub struct PostgresDispatchPublisher {
    pool: PgPool,
    destinations: Arc<DestinationResolver>,
}

impl PostgresDispatchPublisher {
    /// Creates the queue table when missing: the platform owns that schema.
    pub async fn new(
        pool: PgPool,
        destinations: Arc<DestinationResolver>,
    ) -> Result<Self, fc_queue::QueueError> {
        fc_queue::postgres::PostgresQueue::new(pool.clone(), String::new(), 30)
            .init_schema()
            .await?;
        Ok(Self { pool, destinations })
    }
}

#[async_trait]
impl DispatchPublisher for PostgresDispatchPublisher {
    async fn publish(&self, items: Vec<PublishItem>) -> PublishOutcome {
        // Resolve every destination first: a name that cannot be composed is
        // a configuration fault, and Go reports the whole batch unpublished.
        let mut named = Vec::with_capacity(items.len());
        for item in &items {
            match self
                .destinations
                .destination(&item.destination_input())
                .await
            {
                Ok(name) => named.push(name),
                Err(e) => {
                    return PublishOutcome::all(
                        &items,
                        format!("resolve dispatch destination: {e}"),
                    )
                }
            }
        }
        let mut queues: HashMap<String, fc_queue::postgres::PostgresQueue> = HashMap::new();
        for (i, (item, name)) in items.iter().zip(named).enumerate() {
            let queue = queues.entry(name.clone()).or_insert_with(|| {
                fc_queue::postgres::PostgresQueue::new(self.pool.clone(), name.clone(), 30)
            });
            if let Err(e) = queue.publish(item.message.clone()).await {
                // Everything from here on is unpublished; everything before
                // it is durably queued and must not be reverted.
                return PublishOutcome {
                    unpublished: items[i..].iter().map(|x| x.job_id.clone()).collect(),
                    error: Some(format!("postgres dispatch publish: {e}")),
                };
            }
        }
        debug!(
            count = items.len(),
            "dispatch batch published to the postgres broker"
        );
        PublishOutcome::default()
    }

    fn describe(&self) -> String {
        "per-tenant Postgres queues (queue_messages)".to_string()
    }
}

// ── One fixed queue ─────────────────────────────────────────────────────

/// Publishes every job to one queue, in claim order (fc-dev's embedded
/// router consumes exactly one queue).
pub struct SingleQueuePublisher {
    inner: Arc<dyn QueuePublisher>,
}

impl SingleQueuePublisher {
    pub fn new(inner: Arc<dyn QueuePublisher>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl DispatchPublisher for SingleQueuePublisher {
    async fn publish(&self, items: Vec<PublishItem>) -> PublishOutcome {
        for (i, item) in items.iter().enumerate() {
            if let Err(e) = self.inner.publish(item.message.clone()).await {
                return PublishOutcome {
                    unpublished: items[i..].iter().map(|x| x.job_id.clone()).collect(),
                    error: Some(e.to_string()),
                };
            }
        }
        PublishOutcome::default()
    }

    fn describe(&self) -> String {
        format!("the local queue {:?}", self.inner.identifier())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(group: Option<&str>) -> Message {
        Message {
            id: "job1".into(),
            pool_code: "acme-DEFAULT-POOL".into(),
            auth_token: Some("tok".into()),
            signing_secret: None,
            mediation_type: MediationType::HTTP,
            mediation_target: "http://fc-platform:8080/api/dispatch/process".into(),
            message_group_id: group.map(str::to_string),
            high_priority: false,
            dispatch_mode: DispatchMode::NextOnError,
            dispatch_mode_specified: true,
        }
    }

    /// Go's `json.Marshal(common.Message)`: field order as declared, empty
    /// optionals omitted.
    #[test]
    fn wire_body_is_gos_shape() {
        assert_eq!(
            wire_body(&message(Some("orders-1"))),
            r#"{"id":"job1","poolCode":"acme-DEFAULT-POOL","authToken":"tok","mediationType":"HTTP","mediationTarget":"http://fc-platform:8080/api/dispatch/process","messageGroupId":"orders-1","dispatchMode":"NEXT_ON_ERROR"}"#
        );
        assert_eq!(
            wire_body(&message(None)),
            r#"{"id":"job1","poolCode":"acme-DEFAULT-POOL","authToken":"tok","mediationType":"HTTP","mediationTarget":"http://fc-platform:8080/api/dispatch/process","dispatchMode":"NEXT_ON_ERROR"}"#
        );
    }

    /// Both routers decode it.
    #[test]
    fn wire_body_round_trips_through_the_rust_message() {
        let decoded: Message = serde_json::from_str(&wire_body(&message(Some("g")))).unwrap();
        assert_eq!(decoded.auth_token.as_deref(), Some("tok"));
        assert_eq!(decoded.message_group_id.as_deref(), Some("g"));
        assert_eq!(decoded.dispatch_mode, DispatchMode::NextOnError);
        assert!(decoded.dispatch_mode_specified);
    }

    #[test]
    fn group_id_falls_back_to_the_job_id() {
        let item = |g: Option<&str>| PublishItem {
            job_id: "job1".into(),
            client_id: None,
            subscription_id: None,
            queue: None,
            message: message(g),
        };
        assert_eq!(item(Some("g")).group_id(), "g");
        assert_eq!(item(None).group_id(), "job1");
        assert_eq!(item(Some("")).group_id(), "job1");
    }
}
