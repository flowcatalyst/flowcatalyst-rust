//! Delete ScheduledJob — hard removes the definition row. Instances + logs
//! remain (history retention is partition-driven). Prefer Archive for normal
//! lifecycle; Delete is for cleanup of mistakes / abandoned definitions.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::events::ScheduledJobDeleted;
use crate::scheduled_job::entity::ScheduledJob;
use crate::scheduled_job::ScheduledJobRepository;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteScheduledJobCommand {
    pub scheduled_job_id: String,
}

impl crate::usecase::AuditMasked for DeleteScheduledJobCommand {}

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

    async fn authorize(&self, _: &Self::Command, _: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        cmd: Self::Command,
        ctx: ExecutionContext,
    ) -> UseCaseResult<Self::Event> {
        let (job, event) = match self.prepare(&cmd, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit_delete(&job, &*self.repo, event, &cmd)
            .await
    }
}

impl<U: UnitOfWork> DeleteScheduledJobUseCase<U> {
    async fn prepare(
        &self,
        cmd: &DeleteScheduledJobCommand,
        ctx: &ExecutionContext,
    ) -> Result<(ScheduledJob, ScheduledJobDeleted), UseCaseError> {
        let job = self
            .repo
            .find_by_id(&cmd.scheduled_job_id)
            .await
            .or_not_found(
                "SCHEDULED_JOB_NOT_FOUND",
                format!("ScheduledJob '{}' not found", cmd.scheduled_job_id),
            )?;

        let event = ScheduledJobDeleted::new(ctx, &job.id, job.client_id.as_deref(), &job.code);
        Ok((job, event))
    }
}
