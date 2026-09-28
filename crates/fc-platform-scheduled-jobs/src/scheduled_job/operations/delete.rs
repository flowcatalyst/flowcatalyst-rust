//! Delete ScheduledJob — hard removes the definition row. Instances + logs
//! remain (history retention is partition-driven). Prefer Archive for normal
//! lifecycle; Delete is for cleanup of mistakes / abandoned definitions.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::events::ScheduledJobDeleted;
use crate::scheduled_job::ScheduledJobRepository;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteScheduledJobCommand {
    pub scheduled_job_id: String,
}

impl fc_platform_core::usecase::AuditMasked for DeleteScheduledJobCommand {}

pub struct DeleteScheduledJobUseCase<U: UnitOfWork> {
    repo: Arc<ScheduledJobRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteScheduledJobUseCase<U> {
    pub fn new(repo: Arc<ScheduledJobRepository>, unit_of_work: Arc<U>) -> Self {
        Self { repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteScheduledJobUseCase<U> {
    type Command = DeleteScheduledJobCommand;
    type Event = ScheduledJobDeleted;

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
        command: &DeleteScheduledJobCommand,
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

        let event = ScheduledJobDeleted::new(&ctx, &job.id, &job.code);

        self.unit_of_work
            .commit_delete(&job, &*self.repo, event, &cmd)
            .await
    }
}
