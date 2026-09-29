//! Archive Process Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ProcessArchived;
use crate::process::repository::ProcessRepository;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveProcessCommand {
    pub process_id: String,
}

impl AuditMasked for ArchiveProcessCommand {}

pub struct ArchiveProcessUseCase<U: UnitOfWork> {
    process_repo: Arc<ProcessRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> ArchiveProcessUseCase<U> {
    pub fn new(process_repo: Arc<ProcessRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            process_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for ArchiveProcessUseCase<U> {
    type Command = ArchiveProcessCommand;
    type Event = ProcessArchived;

    async fn validate(&self, command: &ArchiveProcessCommand) -> Result<(), UseCaseError> {
        if command.process_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "PROCESS_ID_REQUIRED",
                "Process ID is required",
            ));
        }
        Ok(())
    }

    /// A process has no client or owner to reach (Go authorizes nothing beyond
    /// the permission). The handler checks `can_write_processes` before the body; asserted here
    /// too so any other caller needs it.
    async fn authorize(
        &self,
        _command: &ArchiveProcessCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(checks::can_write_processes(ctx.caller())?)
    }

    async fn execute(
        &self,
        command: ArchiveProcessCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<ProcessArchived>, UseCaseError> {
        let mut process = self
            .process_repo
            .find_by_id(&command.process_id)
            .await
            .or_not_found(
                "PROCESS_NOT_FOUND",
                format!("Process with ID '{}' not found", command.process_id),
            )?;

        // Go's `ArchiveProcess` archives unconditionally: a repeat is a 204.
        process.archive();

        let event = ProcessArchived::new(&ctx, process.id.as_str(), &process.code);

        self.unit_of_work
            .commit(&process, &*self.process_repo, event, &command)
            .await
    }
}
