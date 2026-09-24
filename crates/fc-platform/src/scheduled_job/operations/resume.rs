//! Resume ScheduledJob — flips PAUSED back to ACTIVE.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::events::ScheduledJobResumed;
use crate::scheduled_job::entity::{ScheduledJob, ScheduledJobStatus};
use crate::scheduled_job::ScheduledJobRepository;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeScheduledJobCommand {
    pub scheduled_job_id: String,
}

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

impl<U: UnitOfWork> ResumeScheduledJobUseCase<U> {
    async fn prepare(
        &self,
        cmd: &ResumeScheduledJobCommand,
        ctx: &ExecutionContext,
    ) -> Result<(ScheduledJob, ScheduledJobResumed), UseCaseError> {
        let mut job = self
            .repo
            .find_by_id(&cmd.scheduled_job_id)
            .await
            .or_not_found(
                "SCHEDULED_JOB_NOT_FOUND",
                format!("ScheduledJob '{}' not found", cmd.scheduled_job_id),
            )?;

        if job.status == ScheduledJobStatus::Active {
            return Err(UseCaseError::business_rule(
                "ALREADY_ACTIVE",
                "ScheduledJob is already active",
            ));
        }
        if job.status == ScheduledJobStatus::Archived {
            return Err(UseCaseError::business_rule(
                "ARCHIVED",
                "Cannot resume an archived ScheduledJob",
            ));
        }

        job.resume();
        let event = ScheduledJobResumed::new(ctx, &job.id, job.client_id.as_deref(), &job.code);
        Ok((job, event))
    }
}
