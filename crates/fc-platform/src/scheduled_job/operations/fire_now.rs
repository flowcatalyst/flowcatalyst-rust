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
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
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

    async fn authorize(&self, _: &Self::Command, _: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        cmd: Self::Command,
        ctx: ExecutionContext,
    ) -> UseCaseResult<Self::Event> {
        let event = match self.prepare(&cmd, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work.emit_event(event, &cmd).await
    }
}

impl<U: UnitOfWork> FireScheduledJobUseCase<U> {
    async fn prepare(
        &self,
        cmd: &FireScheduledJobCommand,
        ctx: &ExecutionContext,
    ) -> Result<ScheduledJobFiredManually, UseCaseError> {
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
                "Cannot fire an archived ScheduledJob",
            ));
        }
        // PAUSED jobs are still firable manually — that's the whole point of
        // a manual trigger. The poller skips PAUSED; humans can override.

        let now = Utc::now();
        let instance = ScheduledJobInstance {
            id: crate::TsidGenerator::generate(crate::EntityType::ScheduledJobInstance),
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

        let event = ScheduledJobFiredManually::new(
            ctx,
            &job.id,
            job.client_id.as_deref(),
            &job.code,
            &instance.id,
        );
        Ok(event)
    }
}
