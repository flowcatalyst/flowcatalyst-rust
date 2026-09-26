//! Create Process Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ProcessCreated;
use crate::process::entity::{Process, ProcessCode, ProcessCodeError};
use crate::process::repository::ProcessRepository;
use crate::usecase::{Committed, ExecutionContext, UnitOfWork, UseCase, UseCaseError};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProcessCommand {
    /// Process code: {application}:{subdomain}:{process-name}. Parsed where
    /// the command is built ([`CreateProcessCommand::parse_code`]).
    pub code: ProcessCode,
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

impl CreateProcessCommand {
    /// Parse a requested code for this command, with the validation errors
    /// in their order: `CODE_REQUIRED` for a blank code, then
    /// `NAME_REQUIRED` for a blank name, then `INVALID_CODE_FORMAT`. The
    /// name is looked at only so that a blank name still wins over a
    /// malformed code, as it did when all three were checked in `validate`.
    ///
    /// Handlers call this after their permission check, where they build
    /// the command, so an unauthorised caller gets 403 before any 400.
    pub fn parse_code(code: &str, name: &str) -> Result<ProcessCode, UseCaseError> {
        let parsed = ProcessCode::parse(code);
        if parsed == Err(ProcessCodeError::Required) {
            return Err(UseCaseError::validation(
                "CODE_REQUIRED",
                "Process code is required",
            ));
        }
        if name.trim().is_empty() {
            return Err(name_required());
        }
        parsed.map_err(|e| UseCaseError::validation("INVALID_CODE_FORMAT", e.to_string()))
    }
}

fn name_required() -> UseCaseError {
    UseCaseError::validation("NAME_REQUIRED", "Process name is required")
}

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

    /// The code is a [`ProcessCode`], parsed when the command was built.
    async fn validate(&self, command: &CreateProcessCommand) -> Result<(), UseCaseError> {
        if command.name.trim().is_empty() {
            return Err(name_required());
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
    ) -> Result<Committed<ProcessCreated>, UseCaseError> {
        let existing = self
            .process_repo
            .find_by_code(command.code.as_str())
            .await?;
        if existing.is_some() {
            return Err(UseCaseError::business_rule(
                "CODE_EXISTS",
                format!("Process with code '{}' already exists", command.code),
            ));
        }

        let mut process = Process::new(command.code.clone(), &command.name);
        process.description = command.description.clone();
        process.body = command.body.clone();
        if let Some(d) = &command.diagram_type {
            if !d.trim().is_empty() {
                process.diagram_type = d.clone();
            }
        }
        process.tags = command.tags.clone();
        process.created_by = Some(ctx.principal_id.clone());

        let event = ProcessCreated::new(&ctx, &process.id, &process.code, &process.name);

        self.unit_of_work
            .commit(&process, &*self.process_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_err(code: &str, name: &str) -> (String, String) {
        let e = CreateProcessCommand::parse_code(code, name).unwrap_err();
        (e.code().to_string(), e.message().to_string())
    }

    /// The codes and messages `validate` answered before the code was
    /// parsed, in the same order (code required, name required, format).
    #[test]
    fn parse_code_answers_the_validation_errors_in_order() {
        let err = |code: &str, msg: &str| (code.to_string(), msg.to_string());
        assert_eq!(
            parse_err("", " "),
            err("CODE_REQUIRED", "Process code is required")
        );
        assert_eq!(
            parse_err("a:b", ""),
            err("NAME_REQUIRED", "Process name is required")
        );
        assert_eq!(
            parse_err("a:b", "X"),
            err(
                "INVALID_CODE_FORMAT",
                "Process code must follow format: application:subdomain:process-name"
            )
        );
        assert_eq!(
            parse_err("a:b: ", "X"),
            err(
                "INVALID_CODE_FORMAT",
                "Process code part 'process-name' cannot be empty"
            )
        );
        let cmd = CreateProcessCommand {
            code: CreateProcessCommand::parse_code("a:b:c", "X").unwrap(),
            name: "X".into(),
            description: None,
            body: String::new(),
            diagram_type: None,
            tags: Vec::new(),
        };
        assert!(serde_json::to_string(&cmd)
            .unwrap()
            .contains(r#""code":"a:b:c""#));
    }
}
