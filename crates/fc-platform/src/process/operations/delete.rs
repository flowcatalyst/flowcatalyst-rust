//! Delete Process Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ProcessDeleted;
use crate::process::entity::{Process, ProcessStatus};
use crate::process::repository::ProcessRepository;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteProcessCommand {
    pub process_id: String,
}

pub struct DeleteProcessUseCase<U: UnitOfWork> {
    process_repo: Arc<ProcessRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteProcessUseCase<U> {
    pub fn new(process_repo: Arc<ProcessRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            process_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteProcessUseCase<U> {
    type Command = DeleteProcessCommand;
    type Event = ProcessDeleted;

    async fn validate(&self, command: &DeleteProcessCommand) -> Result<(), UseCaseError> {
        if command.process_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "PROCESS_ID_REQUIRED",
                "Process ID is required",
            ));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &DeleteProcessCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: DeleteProcessCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ProcessDeleted> {
        let (process, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit_delete(&process, &*self.process_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> DeleteProcessUseCase<U> {
    async fn prepare(
        &self,
        command: &DeleteProcessCommand,
        ctx: &ExecutionContext,
    ) -> Result<(Process, ProcessDeleted), UseCaseError> {
        let process = self
            .process_repo
            .find_by_id(&command.process_id)
            .await
            .or_not_found(
                "PROCESS_NOT_FOUND",
                format!("Process with ID '{}' not found", command.process_id),
            )?;

        if process.status != ProcessStatus::Archived {
            return Err(UseCaseError::business_rule(
                "CANNOT_DELETE",
                "Can only delete archived processes",
            ));
        }

        let event = ProcessDeleted::new(ctx, &process.id, &process.code);
        Ok((process, event))
    }
}
