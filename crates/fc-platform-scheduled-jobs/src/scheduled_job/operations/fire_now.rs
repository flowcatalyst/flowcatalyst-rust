//! Manually fire a ScheduledJob right now.
//!
//! Two-phase write: first inserts the instance row directly (platform-
//! infrastructure path, no UoW), then emits a `ScheduledJobFiredManually`
//! domain event via UoW for the audit trail. Order matters — if the
//! infrastructure insert fails, no audit row is written; if the audit emit
//! fails, the instance still exists and the dispatcher will deliver it.
//!
//! The actual webhook delivery happens asynchronously via the existing
//! cron-poller's queue path — the use case only enqueues; it does not call out.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::events::ScheduledJobFiredManually;
use crate::scheduled_job::entity::{
    InstanceStatus, ScheduledJobInstance, ScheduledJobStatus, TriggerKind,
};
use crate::scheduled_job::{ScheduledJobInstanceRepository, ScheduledJobRepository};
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FireScheduledJobCommand {
    pub scheduled_job_id: String,
    /// Optional correlation id stamped on the resulting instance for tracing
    /// across the pipeline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

impl fc_platform_core::usecase::AuditMasked for FireScheduledJobCommand {}

pub struct FireScheduledJobUseCase<U: UnitOfWork> {
    repo: Arc<ScheduledJobRepository>,
    instance_repo: Arc<ScheduledJobInstanceRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> FireScheduledJobUseCase<U> {
    pub fn new(
        repo: Arc<ScheduledJobRepository>,
        instance_repo: Arc<ScheduledJobInstanceRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            repo,
            instance_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for FireScheduledJobUseCase<U> {
    type Command = FireScheduledJobCommand;
    type Event = ScheduledJobFiredManually;

    async fn validate(&self, cmd: &Self::Command) -> Result<(), UseCaseError> {
        if cmd.scheduled_job_id.trim().is_empty() {
            return Err(UseCaseError::validation("ID_REQUIRED", "ID required"));
        }
        Ok(())
    }

    /// Go `CheckScopeAccess` on the stored job (Go checks it post-load): a
    /// client's job needs that client, a platform one anchor scope (403
    /// `SCOPE_FORBIDDEN`). A missing job is `execute`'s 404.
    async fn authorize(
        &self,
        command: &FireScheduledJobCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        if let Some(job) = self.repo.find_by_id(&command.scheduled_job_id).await? {
            fc_platform_core::shared::caller_reach::check_scope_access(
                ctx.caller(),
                job.client_id.as_deref(),
            )?;
        }
        Ok(())
    }

    async fn execute(
        &self,
        cmd: Self::Command,
        ctx: ExecutionContext,
    ) -> Result<Committed<Self::Event>, UseCaseError> {
        let job = self
            .repo
            .find_by_id(&cmd.scheduled_job_id)
            .await
            .or_not_found(
                "SCHEDULED_JOB_NOT_FOUND",
                format!("ScheduledJob '{}' not found", cmd.scheduled_job_id),
            )?;

        if job.status == ScheduledJobStatus::Archived {
            return Err(UseCaseError::business_rule(
                "ARCHIVED",
                "Archived jobs cannot be fired",
            ));
        }
        // PAUSED jobs are still firable manually — that's the whole point of
        // a manual trigger. The poller skips PAUSED; humans can override.

        let now = Utc::now();
        let instance = ScheduledJobInstance {
            id: fc_platform_core::shared::tsid::generate(
                fc_platform_core::shared::tsid::EntityType::ScheduledJobInstance,
            ),
            scheduled_job_id: job.id.clone(),
            client_id: job.client_id.clone(),
            job_code: job.code.clone(),
            trigger_kind: TriggerKind::Manual,
            scheduled_for: None,
            fired_at: now,
            delivered_at: None,
            completed_at: None,
            status: InstanceStatus::Queued,
            delivery_attempts: 0,
            delivery_error: None,
            completion_status: None,
            completion_result: None,
            correlation_id: cmd.correlation_id.clone(),
            created_at: now,
        };

        if let Err(e) = self.instance_repo.insert(&instance).await {
            return Err(UseCaseError::commit(format!(
                "Failed to insert instance row: {}",
                e
            )));
        }

        let event = ScheduledJobFiredManually::new(&ctx, &job.id, &job.code, &instance.id);

        self.unit_of_work.emit_event(event, &cmd).await
    }
}
