//! Delete Process Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ProcessDeleted;
use crate::process::entity::ProcessStatus;
use crate::process::repository::ProcessRepository;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteProcessCommand {
    pub process_id: String,
}

impl fc_platform_core::usecase::AuditMasked for DeleteProcessCommand {}

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

    /// A process has no client or owner to reach (Go authorizes nothing beyond
    /// the permission). The handler checks `can_delete_processes` before the body; asserted here
    /// too so any other caller needs it.
    async fn authorize(
        &self,
        _command: &DeleteProcessCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(
            fc_platform_core::shared::authorization_service::checks::can_delete_processes(
                ctx.caller(),
            )?,
        )
    }

    async fn execute(
        &self,
        command: DeleteProcessCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<ProcessDeleted>, UseCaseError> {
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

        let event = ProcessDeleted::new(&ctx, &process.id, &process.code);

        self.unit_of_work
            .commit_delete(&process, &*self.process_repo, event, &command)
            .await
    }
}
