//! Scheduled-job webhook dispatcher.
//!
//! Drains QUEUED `ScheduledJobInstance` rows, POSTs the envelope to the
//! owning job's `target_url`, and transitions each instance to DELIVERED
//! or DELIVERY_FAILED. Any 2xx is delivered, as Java's `JobDispatcher`
//! (`:150-161`) has it. Retry on transient failure is implicit: any other
//! response sets the instance back to QUEUED so the next dispatcher tick
//! picks it up again — until `delivery_max_attempts` is reached, at which
//! point the instance is marked DELIVERY_FAILED (terminal).
//!
//! A job that names an application is delivered with that application's
//! outbound credentials (Java `JobDispatcher.applyCredentials`, `:217-275`):
//! `Authorization: Bearer` for its token and the `X-FlowCatalyst-Timestamp`
//! / `X-FlowCatalyst-Signature` pair for its signing secret, signed over the
//! exact body sent. A function's schedules name the function's application,
//! so the function host's `webhook` endpoint can verify them.
//!
//! Per CLAUDE.md, all writes here bypass UoW (platform-infrastructure path).

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use crate::scheduled_job::entity::{InstanceStatus, ScheduledJob, ScheduledJobInstance};
use crate::scheduled_job::scheduler::config::ScheduledJobSchedulerConfig;
use crate::scheduled_job::{
    InstanceListFilters, ScheduledJobInstanceRepository, ScheduledJobRepository,
};
use crate::service_account::outbound_credentials::{
    OutboundCredentials, OutboundCredentialsResolver,
};
use crate::shared::webhook_signer;

/// Webhook envelope sent to the SDK. Stable shape — the `payload` field
/// passes through whatever the job stores.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebhookEnvelope<'a> {
    job_id: &'a str,
    job_code: &'a str,
    instance_id: &'a str,
    scheduled_for: Option<chrono::DateTime<chrono::Utc>>,
    fired_at: chrono::DateTime<chrono::Utc>,
    trigger_kind: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    correlation_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<&'a serde_json::Value>,
    /// True when the SDK is expected to call back via `/instances/:id/complete`.
    tracks_completion: bool,
    /// Hint for the SDK's own runtime timeout.
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout_seconds: Option<i32>,
}

pub struct ScheduledJobDispatcher {
    config: ScheduledJobSchedulerConfig,
    repo: Arc<ScheduledJobRepository>,
    instance_repo: Arc<ScheduledJobInstanceRepository>,
    http_client: reqwest::Client,
    /// `None` delivers every job unsigned.
    credentials: Option<Arc<OutboundCredentialsResolver>>,
    shutdown: broadcast::Receiver<()>,
}

impl ScheduledJobDispatcher {
    pub fn new(
        config: ScheduledJobSchedulerConfig,
        repo: Arc<ScheduledJobRepository>,
        instance_repo: Arc<ScheduledJobInstanceRepository>,
        http_client: reqwest::Client,
        shutdown: broadcast::Receiver<()>,
    ) -> Self {
        Self {
            config,
            repo,
            instance_repo,
            http_client,
            credentials: None,
            shutdown,
        }
    }

    /// Sign deliveries with each job's application's credentials.
    pub fn with_credentials(mut self, credentials: Arc<OutboundCredentialsResolver>) -> Self {
        self.credentials = Some(credentials);
        self
    }

    pub async fn run(mut self) {
        info!(
            interval_seconds = self.config.dispatch_interval.as_secs(),
            batch = self.config.dispatch_batch_size,
            "Scheduled-job dispatcher started"
        );
        let mut ticker = tokio::time::interval(self.config.dispatch_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    if let Err(e) = self.tick().await {
                        error!(error = %e, "Scheduled-job dispatcher tick failed");
                    }
                }
                _ = self.shutdown.recv() => {
                    info!("Scheduled-job dispatcher shutting down");
                    return;
                }
            }
        }
    }

    async fn tick(&self) -> crate::shared::error::Result<()> {
        let instances = self
            .instance_repo
            .list(&InstanceListFilters {
                status: Some(InstanceStatus::Queued),
                limit: Some(self.config.dispatch_batch_size),
                ..Default::default()
            })
            .await?;

        if instances.is_empty() {
            return Ok(());
        }
        debug!(
            count = instances.len(),
            "Dispatching queued scheduled-job instances"
        );

        // Batch-load jobs to avoid N+1.
        let mut job_cache: HashMap<String, Option<ScheduledJob>> = HashMap::new();
        for inst in &instances {
            if !job_cache.contains_key(&inst.scheduled_job_id) {
                let job = self.repo.find_by_id(&inst.scheduled_job_id).await?;
                job_cache.insert(inst.scheduled_job_id.clone(), job);
            }
        }

        let mut delivered = 0usize;
        let mut failed = 0usize;
        let mut requeued = 0usize;
        for inst in instances {
            let Some(Some(job)) = job_cache.get(&inst.scheduled_job_id) else {
                // Job was deleted between insert and dispatch. Mark terminal.
                let _ = self
                    .instance_repo
                    .mark_delivery_failed(
                        &inst.id,
                        inst.created_at,
                        "ScheduledJob no longer exists",
                        true,
                    )
                    .await;
                failed += 1;
                continue;
            };

            match self.dispatch_one(job, &inst).await {
                DispatchOutcome::Delivered => delivered += 1,
                DispatchOutcome::Failed => failed += 1,
                DispatchOutcome::Requeued => requeued += 1,
            }
        }
        if delivered > 0 || failed > 0 || requeued > 0 {
            info!(
                delivered,
                failed, requeued, "Scheduled-job dispatch tick completed"
            );
        }
        Ok(())
    }

    async fn dispatch_one(
        &self,
        job: &ScheduledJob,
        inst: &ScheduledJobInstance,
    ) -> DispatchOutcome {
        let Some(target_url) = &job.target_url else {
            warn!(job_id = %job.id, instance_id = %inst.id, "ScheduledJob has no target_url; marking instance DELIVERY_FAILED");
            let _ = self
                .instance_repo
                .mark_delivery_failed(&inst.id, inst.created_at, "No target_url configured", true)
                .await;
            return DispatchOutcome::Failed;
        };

        if let Err(e) = self
            .instance_repo
            .mark_in_flight(&inst.id, inst.created_at)
            .await
        {
            error!(instance_id = %inst.id, error = %e, "Failed to mark instance IN_FLIGHT");
            return DispatchOutcome::Failed;
        }

        let envelope = WebhookEnvelope {
            job_id: &job.id,
            job_code: &job.code,
            instance_id: &inst.id,
            scheduled_for: inst.scheduled_for,
            fired_at: inst.fired_at,
            trigger_kind: inst.trigger_kind.as_str(),
            correlation_id: inst.correlation_id.as_deref(),
            payload: job.payload.as_ref(),
            tracks_completion: job.tracks_completion,
            timeout_seconds: job.timeout_seconds,
        };

        let body = match serde_json::to_vec(&envelope) {
            Ok(b) => b,
            Err(e) => {
                let err = format!("Envelope serialisation failed: {e}");
                return self
                    .handle_failure(job, inst, inst.delivery_attempts + 1, &err)
                    .await;
            }
        };
        let credentials = self.credentials_for(job).await;
        let request = signed_request(
            self.http_client.post(target_url),
            credentials.as_ref(),
            chrono::Utc::now(),
            body,
        );
        let result = request.send().await;

        let attempts_after_inc = inst.delivery_attempts + 1;
        match result {
            Ok(resp) if resp.status().is_success() => {
                if let Err(e) = self
                    .instance_repo
                    .mark_delivered(&inst.id, inst.created_at)
                    .await
                {
                    error!(instance_id = %inst.id, error = %e, "Failed to mark DELIVERED");
                    return DispatchOutcome::Failed;
                }
                DispatchOutcome::Delivered
            }
            Ok(resp) => {
                let status = resp.status();
                // Only as far as Go's 500-byte snippet: the rest of a slow or
                // huge body is never read (the scheduler loop waits on it).
                let snippet = crate::shared::capped_body::read_capped(
                    resp,
                    crate::shared::capped_body::SCHEDULED_JOB_ERROR_SNIPPET_CAP,
                )
                .await
                .text();
                let err = format!("HTTP {} (expected 2xx): {}", status, snippet);
                self.handle_failure(job, inst, attempts_after_inc, &err)
                    .await
            }
            Err(e) => {
                let err = format!("Network/HTTP error: {}", e);
                self.handle_failure(job, inst, attempts_after_inc, &err)
                    .await
            }
        }
    }

    /// The job's application's credentials; `None` (unsigned) when it names
    /// no application, has no active account, or the lookup fails.
    async fn credentials_for(&self, job: &ScheduledJob) -> Option<OutboundCredentials> {
        let resolver = self.credentials.as_ref()?;
        let Some(application_id) = job.application_id.as_deref() else {
            debug!(job_id = %job.id, "Scheduled job has no application; delivering unsigned");
            return None;
        };
        match resolver.for_application(application_id).await {
            Ok(Some(creds)) if !creds.is_empty() => {
                if creds.signing_secret.is_none() {
                    warn!(job_id = %job.id, "Scheduled job's application has a bearer token but no signing secret; delivering unsigned");
                } else if creds.token.is_none() {
                    warn!(job_id = %job.id, "Scheduled job's application has a signing secret but no bearer token; delivering with the signature only");
                }
                Some(creds)
            }
            Ok(_) => {
                warn!(job_id = %job.id, "Scheduled job's application has no active service account credentials; delivering unsigned");
                None
            }
            Err(e) => {
                warn!(job_id = %job.id, error = %e, "Outbound credentials lookup failed for scheduled job; delivering unsigned");
                None
            }
        }
    }

    async fn handle_failure(
        &self,
        job: &ScheduledJob,
        inst: &ScheduledJobInstance,
        attempts_after_inc: i32,
        error: &str,
    ) -> DispatchOutcome {
        let terminal = attempts_after_inc >= job.delivery_max_attempts;
        if let Err(e) = self
            .instance_repo
            .mark_delivery_failed(&inst.id, inst.created_at, error, terminal)
            .await
        {
            error!(instance_id = %inst.id, error = %e, "Failed to record delivery failure");
            return DispatchOutcome::Failed;
        }
        if terminal {
            warn!(
                instance_id = %inst.id,
                attempts = attempts_after_inc,
                max = job.delivery_max_attempts,
                error = %error,
                "Scheduled-job delivery exhausted retries"
            );
            DispatchOutcome::Failed
        } else {
            debug!(
                instance_id = %inst.id,
                attempts = attempts_after_inc,
                max = job.delivery_max_attempts,
                error = %error,
                "Scheduled-job delivery failed; requeued"
            );
            DispatchOutcome::Requeued
        }
    }
}

/// The delivery request: JSON `body`, the bearer token when there is one,
/// and the signature headers over `body` at `at` when there is a secret.
pub fn signed_request(
    request: reqwest::RequestBuilder,
    credentials: Option<&OutboundCredentials>,
    at: chrono::DateTime<chrono::Utc>,
    body: Vec<u8>,
) -> reqwest::RequestBuilder {
    let mut request = request.header(reqwest::header::CONTENT_TYPE, "application/json");
    if let Some(creds) = credentials {
        if let Some(token) = &creds.token {
            request = request.header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"));
        }
        if let Some(secret) = &creds.signing_secret {
            for (name, value) in webhook_signer::signature_headers(secret, at, &body) {
                request = request.header(name, value);
            }
        }
    }
    request.body(body)
}

enum DispatchOutcome {
    Delivered,
    Failed,
    Requeued,
}
