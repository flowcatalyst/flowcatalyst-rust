//! Sync Processes Use Case
//!
//! Bulk creates/updates/deletes processes from an application SDK.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ProcessesSynced;
use crate::process::entity::{Process, ProcessSource};
use crate::process::repository::ProcessRepository;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProcessInput {
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Diagram body (typically Mermaid source).
    #[serde(default)]
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagram_type: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProcessesCommand {
    pub application_code: String,
    pub processes: Vec<SyncProcessInput>,
    #[serde(default)]
    pub remove_unlisted: bool,
}

pub struct SyncProcessesUseCase<U: UnitOfWork> {
    process_repo: Arc<ProcessRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> SyncProcessesUseCase<U> {
    pub fn new(process_repo: Arc<ProcessRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            process_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for SyncProcessesUseCase<U> {
    type Command = SyncProcessesCommand;
    type Event = ProcessesSynced;

    async fn validate(&self, command: &SyncProcessesCommand) -> Result<(), UseCaseError> {
        if command.application_code.trim().is_empty() {
            return Err(UseCaseError::validation(
                "APPLICATION_CODE_REQUIRED",
                "Application code is required",
            ));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &SyncProcessesCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: SyncProcessesCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ProcessesSynced> {
        let event = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work.emit_event(event, &command).await
    }
}

impl<U: UnitOfWork> SyncProcessesUseCase<U> {
    async fn prepare(
        &self,
        command: &SyncProcessesCommand,
        ctx: &ExecutionContext,
    ) -> Result<ProcessesSynced, UseCaseError> {
        let existing = self
            .process_repo
            .find_by_application(&command.application_code)
            .await?;

        let mut created = 0u32;
        let mut updated = 0u32;
        let mut deleted = 0u32;
        let mut synced_codes: Vec<String> = Vec::new();

        for input in &command.processes {
            synced_codes.push(input.code.clone());
            match existing.iter().find(|p| p.code == input.code) {
                Some(existing_p) => {
                    if existing_p.source == ProcessSource::Api
                        || existing_p.source == ProcessSource::Code
                    {
                        let mut up = existing_p.clone();
                        up.name = input.name.clone();
                        up.description = input.description.clone();
                        up.body = input.body.clone();
                        if let Some(d) = &input.diagram_type {
                            if !d.trim().is_empty() {
                                up.diagram_type = d.clone();
                            }
                        }
                        up.tags = input.tags.clone();
                        up.updated_at = chrono::Utc::now();
                        if let Err(e) = self.process_repo.update(&up).await {
                            return Err(UseCaseError::commit(format!(
                                "Failed to update process '{}': {}",
                                input.code, e
                            )));
                        }
                        updated += 1;
                    }
                }
                None => {
                    let mut p = Process::new(&input.code, &input.name)
                        .map_err(|e| UseCaseError::validation("INVALID_PROCESS_CODE", e))?;
                    p.source = ProcessSource::Api;
                    p.description = input.description.clone();
                    p.body = input.body.clone();
                    if let Some(d) = &input.diagram_type {
                        if !d.trim().is_empty() {
                            p.diagram_type = d.clone();
                        }
                    }
                    p.tags = input.tags.clone();
                    if let Err(e) = self.process_repo.insert(&p).await {
                        return Err(UseCaseError::commit(format!(
                            "Failed to create process '{}': {}",
                            input.code, e
                        )));
                    }
                    created += 1;
                }
            }
        }

        if command.remove_unlisted {
            for p in &existing {
                if (p.source == ProcessSource::Api || p.source == ProcessSource::Code)
                    && !synced_codes.contains(&p.code)
                {
                    if let Err(e) = self.process_repo.delete(&p.id).await {
                        return Err(UseCaseError::commit(format!(
                            "Failed to delete process '{}': {}",
                            p.code, e
                        )));
                    }
                    deleted += 1;
                }
            }
        }

        let event = ProcessesSynced::new(
            ctx,
            &command.application_code,
            created,
            updated,
            deleted,
            synced_codes,
        );
        Ok(event)
    }
}
