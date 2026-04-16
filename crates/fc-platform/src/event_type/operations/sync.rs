//! Sync Event Types Use Case
//!
//! Bulk creates/updates/deletes event types from an application SDK.

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::event_type::entity::{EventType, EventTypeSource};
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

        // Process each input event type
        for input in &command.event_types {
            synced_codes.push(input.code.clone());

            let existing_et = existing.iter().find(|et| et.code == input.code);
            match existing_et {
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
                }
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
