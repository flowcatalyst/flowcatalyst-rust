//! Sync Processes Use Case
//!
//! Bulk creates/updates/deletes processes from an application SDK.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::{ProcessCreated, ProcessDeleted, ProcessUpdated, ProcessesSynced};
use crate::process::entity::{Process, ProcessCode, ProcessCodeError, ProcessSource};
use crate::process::repository::ProcessRepository;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, RecordedEvent, UnitOfWork, UseCase, UseCaseError,
};

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

impl AuditMasked for SyncProcessesCommand {}

/// A listed code that is not a process code (checked when the sync reaches
/// it, as it creates the process). The sync keeps its own code and its
/// messages: a blank code reads as a wrong segment count, and an empty
/// segment is not named.
fn invalid_sync_code(e: ProcessCodeError) -> UseCaseError {
    let message = match e {
        ProcessCodeError::Required | ProcessCodeError::WrongSegmentCount => {
            ProcessCodeError::WrongSegmentCount.to_string()
        }
        ProcessCodeError::EmptySegment(_) => "Process code segments cannot be empty".to_string(),
    };
    UseCaseError::validation("INVALID_PROCESS_CODE", message)
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
    ) -> Result<Committed<ProcessesSynced>, UseCaseError> {
        let existing = self
            .process_repo
            .find_by_application(&command.application_code)
            .await?;

        let mut created = 0u32;
        let mut updated = 0u32;
        let mut deleted = 0u32;
        let mut synced_codes: Vec<String> = Vec::new();
        let mut saves: Vec<Process> = Vec::new();
        let mut deletes: Vec<Process> = Vec::new();
        let mut rows: Vec<RecordedEvent> = Vec::new();

        // Plan every row before anything is written (Go's `usecaseop.Sync`):
        // a bad row fails the sync with nothing written.
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
                        rows.push(RecordedEvent::of(&ProcessUpdated::new(
                            &ctx, &up.id, &up.name,
                        ))?);
                        saves.push(up);
                        updated += 1;
                    }
                }
                None => {
                    let code = ProcessCode::parse(&input.code).map_err(invalid_sync_code)?;
                    let mut p = Process::new(code, &input.name);
                    p.source = ProcessSource::Api;
                    p.description = input.description.clone();
                    p.body = input.body.clone();
                    if let Some(d) = &input.diagram_type {
                        if !d.trim().is_empty() {
                            p.diagram_type = d.clone();
                        }
                    }
                    p.tags = input.tags.clone();
                    rows.push(RecordedEvent::of(&ProcessCreated::new(
                        &ctx, &p.id, &p.code, &p.name,
                    ))?);
                    saves.push(p);
                    created += 1;
                }
            }
        }

        if command.remove_unlisted {
            for p in &existing {
                if (p.source == ProcessSource::Api || p.source == ProcessSource::Code)
                    && !synced_codes.contains(&p.code)
                {
                    rows.push(RecordedEvent::of(&ProcessDeleted::new(
                        &ctx, &p.id, &p.code,
                    ))?);
                    deletes.push(p.clone());
                    deleted += 1;
                }
            }
        }

        let event = ProcessesSynced {
            metadata: ProcessesSynced::metadata_for(&ctx, &command.application_code),
            application_code: command.application_code.clone(),
            created,
            updated,
            deleted,
            synced_codes,
        };

        // Go's usecaseop.Sync: the rows, a created/updated/deleted event per
        // synced process, then the rollup, in one transaction.
        self.unit_of_work
            .commit_sync(&*self.process_repo, &saves, &deletes, rows, event, &command)
            .await
    }
}
