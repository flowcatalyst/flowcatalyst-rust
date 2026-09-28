//! Finalise Schema Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::SchemaFinalised;
use crate::event_type::entity::SpecVersionStatus;
use crate::usecase::{Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError};
use crate::EventTypeRepository;

/// Command for finalising a schema version.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinaliseSchemaCommand {
    /// Event type ID
    pub event_type_id: String,

    /// Version to finalise (e.g. "1.0")
    pub version: String,
}

impl crate::usecase::AuditMasked for FinaliseSchemaCommand {}

/// Use case for finalising a schema version (FINALISING → CURRENT).
pub struct FinaliseSchemaUseCase<U: UnitOfWork> {
    event_type_repo: Arc<EventTypeRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> FinaliseSchemaUseCase<U> {
    pub fn new(event_type_repo: Arc<EventTypeRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            event_type_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for FinaliseSchemaUseCase<U> {
    type Command = FinaliseSchemaCommand;
    type Event = SchemaFinalised;

    async fn validate(&self, command: &FinaliseSchemaCommand) -> Result<(), UseCaseError> {
        if command.event_type_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "EVENT_TYPE_ID_REQUIRED",
                "Event type ID is required",
            ));
        }
        if command.version.trim().is_empty() {
            return Err(UseCaseError::validation(
                "VERSION_REQUIRED",
                "Schema version is required",
            ));
        }
        Ok(())
    }

    /// Go `CheckScopeAccess` on the stored event type (Go checks it post-load):
    /// a client's type needs that client, a platform one anchor scope (403
    /// `SCOPE_FORBIDDEN`). A missing type is `execute`'s 404. Holds for the
    /// `/api`, `/bff` and fc-web routes alike.
    async fn authorize(
        &self,
        command: &FinaliseSchemaCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        if let Some(event_type) = self
            .event_type_repo
            .find_by_id(&command.event_type_id)
            .await?
        {
            crate::shared::caller_reach::check_scope_access(
                ctx.caller(),
                event_type.client_id.as_deref(),
            )?;
        }
        Ok(())
    }

    async fn execute(
        &self,
        command: FinaliseSchemaCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<SchemaFinalised>, UseCaseError> {
        let mut event_type = self
            .event_type_repo
            .find_by_id(&command.event_type_id)
            .await
            .or_not_found(
                "EVENT_TYPE_NOT_FOUND",
                format!("Event type with ID '{}' not found", command.event_type_id),
            )?;

        // Find target version
        let target_idx = event_type
            .spec_versions
            .iter()
            .position(|sv| sv.version == command.version)
            .ok_or_else(|| {
                UseCaseError::not_found(
                    "VERSION_NOT_FOUND",
                    format!("Schema version '{}' not found", command.version),
                )
            })?;

        // Business rule: must be in FINALISING status
        if event_type.spec_versions[target_idx].status != SpecVersionStatus::Finalising {
            return Err(UseCaseError::business_rule(
                "NOT_FINALISING",
                format!(
                    "Schema version '{}' is not in FINALISING status",
                    command.version
                ),
            ));
        }

        // Extract major version for auto-deprecation
        let target_major: Option<u32> = command
            .version
            .split('.')
            .next()
            .and_then(|s| s.parse().ok());

        // Auto-deprecate existing CURRENT versions with same major version
        let mut deprecated_version: Option<String> = None;
        if let Some(major) = target_major {
            for sv in &mut event_type.spec_versions {
                if sv.status == SpecVersionStatus::Current {
                    let sv_major: Option<u32> =
                        sv.version.split('.').next().and_then(|s| s.parse().ok());
                    if sv_major == Some(major) {
                        sv.status = SpecVersionStatus::Deprecated;
                        sv.updated_at = chrono::Utc::now();
                        deprecated_version = Some(sv.version.clone());
                    }
                }
            }
        }

        // Finalise target version
        event_type.spec_versions[target_idx].status = SpecVersionStatus::Current;
        event_type.spec_versions[target_idx].updated_at = chrono::Utc::now();
        event_type.updated_at = chrono::Utc::now();

        let event = SchemaFinalised::new(
            &ctx,
            &event_type.id,
            &command.version,
            deprecated_version.as_deref(),
        );

        self.unit_of_work
            .commit(&event_type, &*self.event_type_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = FinaliseSchemaCommand {
            event_type_id: "et-123".to_string(),
            version: "1.0".to_string(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("eventTypeId"));
    }
}
