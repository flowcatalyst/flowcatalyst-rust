//! Pause ScheduledJob — stops further cron firings until resumed. In-flight
//! instances are NOT cancelled; the SDK is responsible for its own runtime.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::events::ScheduledJobPaused;
use crate::scheduled_job::entity::ScheduledJob;
use crate::scheduled_job::ScheduledJobRepository;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PauseScheduledJobCommand {
    pub scheduled_job_id: String,
}

impl crate::usecase::AuditMasked for PauseScheduledJobCommand {}

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
            .commit(&job, &*self.repo, event, &cmd)
            .await
    }
}

impl<U: UnitOfWork> PauseScheduledJobUseCase<U> {
    async fn prepare(
        &self,
        cmd: &PauseScheduledJobCommand,
        ctx: &ExecutionContext,
    ) -> Result<(ScheduledJob, ScheduledJobPaused), UseCaseError> {
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
        let event = ScheduledJobPaused::new(ctx, &job.id, &job.code);
        Ok((job, event))
    }
}
