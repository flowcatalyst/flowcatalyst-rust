//! Update Process Use Case

use async_trait::async_trait;
use fc_platform_core::shared::id::ProcessId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::create::optional_description;
use super::events::ProcessUpdated;
use crate::process::entity::ProcessStatus;
use crate::process::repository::ProcessRepository;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProcessCommand {
    pub process_id: ProcessId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagram_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

impl AuditMasked for UpdateProcessCommand {}

pub struct UpdateProcessUseCase<U: UnitOfWork> {
    process_repo: Arc<ProcessRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateProcessUseCase<U> {
    pub fn new(process_repo: Arc<ProcessRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            process_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for UpdateProcessUseCase<U> {
    type Command = UpdateProcessCommand;
    type Event = ProcessUpdated;

    async fn validate(&self, command: &UpdateProcessCommand) -> Result<(), UseCaseError> {
        if command.process_id.as_str().trim().is_empty() {
            return Err(UseCaseError::validation(
                "PROCESS_ID_REQUIRED",
                "Process ID is required",
            ));
        }
        if command.name.is_none()
            && command.description.is_none()
            && command.body.is_none()
            && command.diagram_type.is_none()
            && command.tags.is_none()
        {
            return Err(UseCaseError::validation(
                "NO_UPDATES",
                "At least one field must be provided for update",
            ));
        }
        Ok(())
    }

    /// A process has no client or owner to reach (Go authorizes nothing beyond
    /// the permission). The handler checks `can_update_processes` before the body; asserted here
    /// too so any other caller needs it.
    async fn authorize(
        &self,
        _command: &UpdateProcessCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(checks::can_update_processes(ctx.caller())?)
    }

    async fn execute(
        &self,
        command: UpdateProcessCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<ProcessUpdated>, UseCaseError> {
        let mut process = self
            .process_repo
            .find_by_id(&command.process_id)
            .await
            .or_not_found(
                "PROCESS_NOT_FOUND",
                format!("Process with ID '{}' not found", command.process_id),
            )?;

        if process.status == ProcessStatus::Archived {
            // Go's code and message (bug 23): the ones the UIs show.
            return Err(UseCaseError::business_rule(
                "PROCESS_ARCHIVED",
                "Archived processes cannot be edited.",
            ));
        }

        // Go applies what is sent; an edit that changes nothing is saved too
        // (it answered 400 `NO_CHANGES` here, a rule Go does not have).
        if let Some(name) = command.name.as_ref() {
            process.name = name.trim().to_string();
        }
        if let Some(desc) = command.description.as_ref() {
            // A blank description is none (Go bug 25: the SPA sends "" for an
            // emptied field).
            process.description = optional_description(Some(desc));
        }
        if let Some(body) = command.body.as_ref() {
            process.body = body.clone();
        }
        if let Some(dt) = command.diagram_type.as_ref() {
            let trimmed = dt.trim();
            if !trimmed.is_empty() {
                process.diagram_type = trimmed.to_string();
            }
        }
        if let Some(tags) = command.tags.as_ref() {
            process.tags = tags.clone();
        }

        process.updated_at = chrono::Utc::now();

        let event = ProcessUpdated::new(&ctx, &process.id, &process.name);

        self.unit_of_work
            .commit(&process, &*self.process_repo, event, &command)
            .await
    }
}
