//! Pause ScheduledJob — stops further cron firings until resumed. In-flight
//! instances are NOT cancelled; the SDK is responsible for its own runtime.

use fc_platform_core::shared::id::ScheduledJobId;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::events::ScheduledJobPaused;
use crate::scheduled_job::ScheduledJobRepository;
use fc_platform_core::shared::caller_reach;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PauseScheduledJobCommand {
    pub scheduled_job_id: ScheduledJobId,
}

impl AuditMasked for PauseScheduledJobCommand {}

pub struct PauseScheduledJobUseCase<U: UnitOfWork> {
    repo: Arc<ScheduledJobRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> PauseScheduledJobUseCase<U> {
    pub fn new(repo: Arc<ScheduledJobRepository>, unit_of_work: Arc<U>) -> Self {
        Self { repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for PauseScheduledJobUseCase<U> {
    type Command = PauseScheduledJobCommand;
    type Event = ScheduledJobPaused;

    async fn validate(&self, cmd: &Self::Command) -> Result<(), UseCaseError> {
        if cmd.scheduled_job_id.as_str().trim().is_empty() {
            return Err(UseCaseError::validation("ID_REQUIRED", "ID required"));
        }
        Ok(())
    }

    /// Go `CheckScopeAccess` on the stored job (Go checks it post-load): a
    /// client's job needs that client, a platform one anchor scope (403
    /// `SCOPE_FORBIDDEN`). A missing job is `execute`'s 404.
    async fn authorize(
        &self,
        command: &PauseScheduledJobCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        if let Some(job) = self.repo.find_by_id(&command.scheduled_job_id).await? {
            caller_reach::check_scope_access(ctx.caller(), job.client_id.as_ref())?;
        }
        Ok(())
    }

    async fn execute(
        &self,
        cmd: Self::Command,
        ctx: ExecutionContext,
    ) -> Result<Committed<Self::Event>, UseCaseError> {
        let mut job = self
            .repo
            .find_by_id(&cmd.scheduled_job_id)
            .await
            .or_not_found(
                "SCHEDULED_JOB_NOT_FOUND",
                format!("ScheduledJob '{}' not found", cmd.scheduled_job_id),
            )?;

        // Go's `PauseScheduledJob` flips the status unconditionally: a
        // repeat is a 204 no-op, not a conflict.
        job.pause();
        let event = ScheduledJobPaused::new(&ctx, &job.id, &job.code);

        self.unit_of_work
            .commit(&job, &*self.repo, event, &cmd)
            .await
    }
}
