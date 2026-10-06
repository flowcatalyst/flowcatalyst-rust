//! Sync Event Types Use Case
//!
//! Bulk creates/updates/deletes event types from an application SDK.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::{EventTypeCreated, EventTypeDeleted, EventTypeUpdated, EventTypesSynced};
use crate::event_type::entity::{
    EventType, EventTypeCode, EventTypeCodeError, EventTypeSource, SpecVersion,
};
use crate::event_type::repository::EventTypeRepository;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, RecordedEvent, UnitOfWork, UseCase, UseCaseError,
};

/// A single event type definition in the sync payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncEventTypeInput {
    /// Full code (application:subdomain:aggregate:event)
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema for the event payload (non-metadata fields)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,
}

/// Command for syncing event types from an application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncEventTypesCommand {
    /// Application code (used as the first segment of event type codes)
    pub application_code: String,
    /// Event types to sync
    pub event_types: Vec<SyncEventTypeInput>,
    /// If true, removes API-sourced event types not in the list
    #[serde(default)]
    pub remove_unlisted: bool,
}

impl AuditMasked for SyncEventTypesCommand {}

/// Result of a sync operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct SyncEventTypesResult {
    pub event: EventTypesSynced,
    pub created: u32,
    pub updated: u32,
    pub deleted: u32,
}

/// A listed code that is not an event type code (checked when the sync
/// reaches it, as it creates the type). The sync keeps its own code and
/// its messages: a blank code reads as a wrong segment count, and an empty
/// segment is not named.
fn invalid_sync_code(e: EventTypeCodeError) -> UseCaseError {
    let message = match e {
        EventTypeCodeError::Required | EventTypeCodeError::WrongSegmentCount => {
            EventTypeCodeError::WrongSegmentCount.to_string()
        }
        EventTypeCodeError::EmptySegment(_) => {
            "Event type code segments cannot be empty".to_string()
        }
    };
    UseCaseError::validation("INVALID_EVENT_TYPE_CODE", message)
}

pub struct SyncEventTypesUseCase<U: UnitOfWork> {
    event_type_repo: Arc<EventTypeRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> SyncEventTypesUseCase<U> {
    pub fn new(event_type_repo: Arc<EventTypeRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            event_type_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for SyncEventTypesUseCase<U> {
    type Command = SyncEventTypesCommand;
    type Event = EventTypesSynced;

    async fn validate(&self, command: &SyncEventTypesCommand) -> Result<(), UseCaseError> {
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
        _command: &SyncEventTypesCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: SyncEventTypesCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<EventTypesSynced>, UseCaseError> {
        // Fetch existing event types for this application
        let existing = self
            .event_type_repo
            .find_by_application(&command.application_code)
            .await?;

        let mut created_count = 0u32;
        let mut updated_count = 0u32;
        let mut deleted_count = 0u32;
        let mut synced_codes: Vec<String> = Vec::new();
        let mut schemas_created = 0u32;
        let mut schemas_updated = 0u32;
        let mut schemas_unchanged = 0u32;
        let mut saves: Vec<EventType> = Vec::new();
        let mut deletes: Vec<EventType> = Vec::new();
        let mut rows: Vec<RecordedEvent> = Vec::new();

        // Plan every row before anything is written, as Go's
        // `usecaseop.Sync` does: a bad row fails the sync with nothing
        // written, and the first bad row in the request is the error.
        for input in &command.event_types {
            synced_codes.push(input.code.clone());

            let mut et = match existing.iter().find(|et| et.code == input.code) {
                Some(et) => {
                    // As Go (eventtype/operations/sync.go): a listed code
                    // that already exists has its name and description
                    // updated, whatever its source.
                    let mut updated = et.clone();
                    updated.name = input.name.clone();
                    updated.description = input.description.clone();
                    updated.updated_at = chrono::Utc::now();
                    rows.push(RecordedEvent::of(&EventTypeUpdated::new(
                        &ctx,
                        &updated.id,
                        &updated.name,
                        updated.description.as_deref(),
                    ))?);
                    updated_count += 1;
                    updated
                }
                None => {
                    let code = EventTypeCode::parse(&input.code).map_err(invalid_sync_code)?;
                    let mut et = EventType::new(code, &input.name);
                    et.source = EventTypeSource::Api;
                    et.description = input.description.clone();
                    rows.push(RecordedEvent::of(&EventTypeCreated::new(&ctx, &et))?);
                    created_count += 1;
                    et
                }
            };

            // The schema, if sent, is the type's spec version "1.0".
            match &input.schema {
                Some(schema) => match et.spec_versions.iter_mut().find(|sv| sv.version == "1.0") {
                    Some(sv) if sv.schema_content.as_ref() != Some(schema) => {
                        sv.schema_content = Some(schema.clone());
                        sv.updated_at = chrono::Utc::now();
                        schemas_updated += 1;
                    }
                    Some(_) => schemas_unchanged += 1,
                    None => {
                        let sv = SpecVersion::new(et.id.clone(), "1.0", Some(schema.clone()));
                        et.spec_versions.push(sv);
                        schemas_created += 1;
                    }
                },
                None => schemas_unchanged += 1,
            }
            saves.push(et);
        }

        // Remove unlisted API-sourced event types.
        if command.remove_unlisted {
            for et in &existing {
                // As Go: only API-sourced rows are ever removed — UI- and
                // CODE-managed rows (the platform's own catalogue) never.
                if et.source == EventTypeSource::Api && !synced_codes.contains(&et.code) {
                    rows.push(RecordedEvent::of(&EventTypeDeleted::new(
                        &ctx, &et.id, &et.code,
                    ))?);
                    deletes.push(et.clone());
                    deleted_count += 1;
                }
            }
        }

        let event = EventTypesSynced {
            metadata: EventTypesSynced::metadata_for(&ctx, &command.application_code),
            application_code: command.application_code.clone(),
            created: created_count,
            updated: updated_count,
            deleted: deleted_count,
            synced_codes,
            schemas_created,
            schemas_updated,
            schemas_unchanged,
        };

        // Go's usecaseop.Sync: the rows, a created/updated/deleted event per
        // synced event type, then the rollup, in one transaction.
        self.unit_of_work
            .commit_sync(
                &*self.event_type_repo,
                &saves,
                &deletes,
                rows,
                event,
                &command,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = SyncEventTypesCommand {
            application_code: "orders".to_string(),
            event_types: vec![SyncEventTypeInput {
                code: "orders:fulfillment:shipment:shipped".to_string(),
                name: "Shipment Shipped".to_string(),
                description: None,
                schema: None,
            }],
            remove_unlisted: false,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("orders"));
    }
}
