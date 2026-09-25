//! Create Process Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ProcessCreated;
use crate::process::entity::Process;
use crate::process::repository::ProcessRepository;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProcessCommand {
    /// Process code: {application}:{subdomain}:{process-name}
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Diagram body (typically Mermaid source).
    #[serde(default)]
    pub body: String,
    /// Defaults to `mermaid` if unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagram_type: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl crate::usecase::AuditMasked for CreateProcessCommand {}

pub struct CreateProcessUseCase<U: UnitOfWork> {
    process_repo: Arc<ProcessRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateProcessUseCase<U> {
    pub fn new(process_repo: Arc<ProcessRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            process_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for CreateProcessUseCase<U> {
    type Command = CreateProcessCommand;
    type Event = ProcessCreated;

    async fn validate(&self, command: &CreateProcessCommand) -> Result<(), UseCaseError> {
        if command.code.trim().is_empty() {
            return Err(UseCaseError::validation(
                "CODE_REQUIRED",
                "Process code is required",
            ));
        }
        if command.name.trim().is_empty() {
            return Err(UseCaseError::validation(
                "NAME_REQUIRED",
                "Process name is required",
            ));
        }
        let parts: Vec<&str> = command.code.split(':').collect();
        if parts.len() != 3 {
            return Err(UseCaseError::validation(
                "INVALID_CODE_FORMAT",
                "Process code must follow format: application:subdomain:process-name",
            ));
        }
        for (i, part) in parts.iter().enumerate() {
            if part.trim().is_empty() {
                let part_name = match i {
                    0 => "application",
                    1 => "subdomain",
                    2 => "process-name",
                    _ => "unknown",
                };
                return Err(UseCaseError::validation(
                    "INVALID_CODE_FORMAT",
                    format!("Process code part '{}' cannot be empty", part_name),
                ));
            }
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &CreateProcessCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: CreateProcessCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ProcessCreated> {
        let existing = match self.process_repo.find_by_code(&command.code).await {
            Ok(found) => found,
            Err(e) => return UseCaseResult::failure(e.into()),
        };
        if existing.is_some() {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "CODE_EXISTS",
                format!("Process with code '{}' already exists", command.code),
            ));
        }

        let process = match Process::new(&command.code, &command.name) {
            Ok(mut p) => {
                p.description = command.description.clone();
                p.body = command.body.clone();
                if let Some(d) = &command.diagram_type {
                    if !d.trim().is_empty() {
                        p.diagram_type = d.clone();
                    }
                }
                p.tags = command.tags.clone();
                p.created_by = Some(ctx.principal_id.clone());
                p
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::validation(
                    "INVALID_CODE_FORMAT",
                    e.to_string(),
                ));
            }
        };

        let event = ProcessCreated::new(&ctx, &process.id, &process.code, &process.name);

        self.unit_of_work
            .commit(&process, &*self.process_repo, event, &command)
            .await
    }
}
