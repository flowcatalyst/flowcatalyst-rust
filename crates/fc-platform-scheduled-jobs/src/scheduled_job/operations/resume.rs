//! Resume ScheduledJob — flips PAUSED back to ACTIVE.

use fc_platform_core::shared::id::ScheduledJobId;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::events::ScheduledJobResumed;
use crate::scheduled_job::ScheduledJobRepository;
use fc_platform_core::shared::caller_reach;
use fc_platform_core::shared::id::OptionIdExt;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeScheduledJobCommand {
    pub scheduled_job_id: ScheduledJobId,
}

impl AuditMasked for ResumeScheduledJobCommand {}

pub struct ResumeScheduledJobUseCase<U: UnitOfWork> {
    repo: Arc<ScheduledJobRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> ResumeScheduledJobUseCase<U> {
    pub fn new(repo: Arc<ScheduledJobRepository>, unit_of_work: Arc<U>) -> Self {
        Self { repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for ResumeScheduledJobUseCase<U> {
    type Command = ResumeScheduledJobCommand;
    type Event = ScheduledJobResumed;

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
        command: &ResumeScheduledJobCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        if let Some(job) = self.repo.find_by_id(&command.scheduled_job_id).await? {
            caller_reach::check_scope_access(ctx.caller(), job.client_id.as_id_str())?;
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

        // Go's `ResumeScheduledJob` flips the status unconditionally.
        job.resume();
        let event = ScheduledJobResumed::new(&ctx, &job.id, &job.code);

        self.unit_of_work
            .commit(&job, &*self.repo, event, &cmd)
            .await
    }
}
