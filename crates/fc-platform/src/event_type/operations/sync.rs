//! Sync Event Types Use Case
//!
//! Bulk creates/updates/deletes event types from an application SDK.

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::event_type::entity::{EventType, EventTypeSource, SpecVersion};
use crate::EventTypeRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use super::events::EventTypesSynced;

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

pub struct SyncEventTypesUseCase<U: UnitOfWork> {
    event_type_repo: Arc<EventTypeRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> SyncEventTypesUseCase<U> {
    pub fn new(event_type_repo: Arc<EventTypeRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self { event_type_repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for SyncEventTypesUseCase<U> {
    type Command = SyncEventTypesCommand;
    type Event = EventTypesSynced;

    async fn validate(&self, command: &SyncEventTypesCommand) -> Result<(), UseCaseError> {
        if command.application_code.trim().is_empty() {
            return Err(UseCaseError::validation(
                "APPLICATION_CODE_REQUIRED", "Application code is required",
            ));
        }
        Ok(())
    }

    async fn authorize(&self, _command: &SyncEventTypesCommand, _ctx: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: SyncEventTypesCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<EventTypesSynced> {
        // Fetch existing event types for this application
        let existing = match self.event_type_repo.find_by_application(&command.application_code).await {
            Ok(list) => list,
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch existing event types: {}", e
                )));
            }
        };

        let mut created_count = 0u32;
        let mut updated_count = 0u32;
        let mut deleted_count = 0u32;
        let mut synced_codes: Vec<String> = Vec::new();
        let mut schemas_created = 0u32;
        let mut schemas_updated = 0u32;
        let mut schemas_unchanged = 0u32;

        // Process each input event type
        for input in &command.event_types {
            synced_codes.push(input.code.clone());

            let current_id: String = match existing.iter().find(|et| et.code == input.code) {
                Some(et) => {
                    // Only update API-sourced event types (skip UI-sourced)
                    if et.source == EventTypeSource::Api || et.source == EventTypeSource::Code {
                        let mut updated = et.clone();
                        updated.name = input.name.clone();
                        updated.description = input.description.clone();
                        updated.updated_at = chrono::Utc::now();
                        if let Err(e) = self.event_type_repo.update(&updated).await {
                            return UseCaseResult::failure(UseCaseError::commit(format!(
                                "Failed to update event type '{}': {}", input.code, e
                            )));
                        }
                        updated_count += 1;
                    }
                    et.id.clone()
                }
                None => {
                    // Create new event type
                    let mut et = match EventType::new(&input.code, &input.name) {
                        Ok(et) => et,
                        Err(e) => {
                            return UseCaseResult::failure(UseCaseError::validation(
                                "INVALID_EVENT_TYPE_CODE", e,
                            ));
                        }
                    };
                    et.source = EventTypeSource::Api;
                    et.description = input.description.clone();
                    if let Err(e) = self.event_type_repo.insert(&et).await {
                        return UseCaseResult::failure(UseCaseError::commit(format!(
                            "Failed to create event type '{}': {}", input.code, e
                        )));
                    }
                    created_count += 1;
                    et.id.clone()
                }
            };

            // Sync schema as SpecVersion "1.0" if provided
            if let Some(ref schema) = input.schema {
                // Re-fetch to get current spec_versions (especially for just-created types)
                let current = match self.event_type_repo.find_by_id(&current_id).await {
                    Ok(Some(et)) => et,
                    Ok(None) => continue, // race: vanished between write and read; skip schema
                    Err(e) => {
                        return UseCaseResult::failure(UseCaseError::commit(format!(
                            "Failed to reload event type '{}': {}", input.code, e
                        )));
                    }
                };

                match current.spec_versions.iter().find(|sv| sv.version == "1.0") {
                    Some(existing_sv) => {
                        if existing_sv.schema_content.as_ref() != Some(schema) {
                            let mut updated_sv = existing_sv.clone();
                            updated_sv.schema_content = Some(schema.clone());
                            updated_sv.updated_at = chrono::Utc::now();
                            if let Err(e) = self.event_type_repo.update_spec_version(&updated_sv).await {
                                return UseCaseResult::failure(UseCaseError::commit(format!(
                                    "Failed to update schema for '{}': {}", input.code, e
                                )));
                            }
                            schemas_updated += 1;
                        } else {
                            schemas_unchanged += 1;
                        }
                    }
                    None => {
                        let sv = SpecVersion::new(&current.id, "1.0", Some(schema.clone()));
                        if let Err(e) = self.event_type_repo.insert_spec_version(&sv).await {
                            return UseCaseResult::failure(UseCaseError::commit(format!(
                                "Failed to insert schema for '{}': {}", input.code, e
                            )));
                        }
                        schemas_created += 1;
                    }
                }
            } else {
                schemas_unchanged += 1;
            }
        }

        // Remove unlisted API-sourced event types
        if command.remove_unlisted {
            for et in &existing {
                if (et.source == EventTypeSource::Api || et.source == EventTypeSource::Code)
                    && !synced_codes.contains(&et.code)
                {
                    if let Err(e) = self.event_type_repo.delete(&et.id).await {
                        return UseCaseResult::failure(UseCaseError::commit(format!(
                            "Failed to delete event type '{}': {}", et.code, e
                        )));
                    }
                    deleted_count += 1;
                }
            }
        }

        let event = EventTypesSynced::new(
            &ctx,
            &command.application_code,
            created_count,
            updated_count,
            deleted_count,
            synced_codes,
            schemas_created,
            schemas_updated,
            schemas_unchanged,
        );

        self.unit_of_work.emit_event(event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = SyncEventTypesCommand {
            application_code: "orders".to_string(),
            event_types: vec![
                SyncEventTypeInput {
                    code: "orders:fulfillment:shipment:shipped".to_string(),
                    name: "Shipment Shipped".to_string(),
                    description: None,
                    schema: None,
                },
            ],
            remove_unlisted: false,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("orders"));
    }
}
