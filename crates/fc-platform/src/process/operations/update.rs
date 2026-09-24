//! Update Process Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ProcessUpdated;
use crate::process::entity::{Process, ProcessStatus};
use crate::process::repository::ProcessRepository;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProcessCommand {
    pub process_id: String,
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
        if command.process_id.trim().is_empty() {
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

    async fn authorize(
        &self,
        _command: &UpdateProcessCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: UpdateProcessCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ProcessUpdated> {
        let (process, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit(&process, &*self.process_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> UpdateProcessUseCase<U> {
    async fn prepare(
        &self,
        command: &UpdateProcessCommand,
        ctx: &ExecutionContext,
    ) -> Result<(Process, ProcessUpdated), UseCaseError> {
        let mut process = self
            .process_repo
            .find_by_id(&command.process_id)
            .await
            .or_not_found(
                "PROCESS_NOT_FOUND",
                format!("Process with ID '{}' not found", command.process_id),
            )?;

        if process.status == ProcessStatus::Archived {
            return Err(UseCaseError::business_rule(
                "CANNOT_UPDATE_ARCHIVED",
                "Cannot update an archived process",
            ));
        }

        let mut changed_name: Option<String> = None;
        let mut changed_description: Option<String> = None;
        let mut body_changed = false;
        let mut changed_tags: Option<Vec<String>> = None;
        let mut any_change = false;

        if let Some(name) = command.name.as_ref() {
            let trimmed = name.trim();
            if trimmed != process.name {
                process.name = trimmed.to_string();
                changed_name = Some(trimmed.to_string());
                any_change = true;
            }
        }
        if let Some(desc) = command.description.as_ref() {
            if process.description.as_deref() != Some(desc.as_str()) {
                process.description = Some(desc.clone());
                changed_description = Some(desc.clone());
                any_change = true;
            }
        }
        if let Some(body) = command.body.as_ref() {
            if &process.body != body {
                process.body = body.clone();
                body_changed = true;
                any_change = true;
            }
        }
        if let Some(dt) = command.diagram_type.as_ref() {
            let trimmed = dt.trim();
            if !trimmed.is_empty() && trimmed != process.diagram_type {
                process.diagram_type = trimmed.to_string();
                any_change = true;
            }
        }
        if let Some(tags) = command.tags.as_ref() {
            if &process.tags != tags {
                process.tags = tags.clone();
                changed_tags = Some(tags.clone());
                any_change = true;
            }
        }

        if !any_change {
            return Err(UseCaseError::validation(
                "NO_CHANGES",
                "No changes detected",
            ));
        }

        process.updated_at = chrono::Utc::now();

        let event = ProcessUpdated {
            metadata: ProcessUpdated::metadata_for(ctx, &process.id),
            process_id: process.id.clone(),
            name: changed_name,
            description: changed_description,
            body_changed: body_changed.then_some(true),
            tags: changed_tags,
        };
        Ok((process, event))
    }
}
