//! Deprecate Schema Use Case

use async_trait::async_trait;
use fc_platform_core::shared::id::EventTypeId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::SchemaDeprecated;
use crate::event_type::entity::SpecVersionStatus;
use crate::event_type::repository::EventTypeRepository;
use fc_platform_core::shared::caller_reach;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for deprecating a schema version.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeprecateSchemaCommand {
    /// Event type ID
    pub event_type_id: EventTypeId,

    /// Version to deprecate (e.g. "1.0")
    pub version: String,
}

impl AuditMasked for DeprecateSchemaCommand {}

/// Use case for deprecating a schema version (CURRENT → DEPRECATED).
pub struct DeprecateSchemaUseCase<U: UnitOfWork> {
    event_type_repo: Arc<EventTypeRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeprecateSchemaUseCase<U> {
    pub fn new(event_type_repo: Arc<EventTypeRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            event_type_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeprecateSchemaUseCase<U> {
    type Command = DeprecateSchemaCommand;
    type Event = SchemaDeprecated;

    async fn validate(&self, command: &DeprecateSchemaCommand) -> Result<(), UseCaseError> {
        if command.event_type_id.as_str().trim().is_empty() {
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
        command: &DeprecateSchemaCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        if let Some(event_type) = self
            .event_type_repo
            .find_by_id(&command.event_type_id)
            .await?
        {
            caller_reach::check_scope_access(ctx.caller(), event_type.client_id.as_deref())?;
        }
        Ok(())
    }

    async fn execute(
        &self,
        command: DeprecateSchemaCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<SchemaDeprecated>, UseCaseError> {
        let mut event_type = self
            .event_type_repo
            .find_by_id(&command.event_type_id)
            .await
            .or_not_found(
                "EVENT_TYPE_NOT_FOUND",
                format!("Event type with ID '{}' not found", command.event_type_id),
            )?;

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

        // Business rule: cannot deprecate FINALISING schemas
        if event_type.spec_versions[target_idx].status == SpecVersionStatus::Finalising {
            return Err(UseCaseError::business_rule(
                "CANNOT_DEPRECATE_FINALISING",
                "Cannot deprecate a schema that is still in FINALISING status",
            ));
        }

        // Business rule: cannot deprecate already deprecated
        if event_type.spec_versions[target_idx].status == SpecVersionStatus::Deprecated {
            return Err(UseCaseError::business_rule(
                "ALREADY_DEPRECATED",
                "Schema version is already deprecated",
            ));
        }

        // Deprecate
        event_type.spec_versions[target_idx].status = SpecVersionStatus::Deprecated;
        event_type.spec_versions[target_idx].updated_at = chrono::Utc::now();
        event_type.updated_at = chrono::Utc::now();

        let event = SchemaDeprecated::new(&ctx, &event_type.id, &command.version);

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
        let cmd = DeprecateSchemaCommand {
            event_type_id: EventTypeId::from_wire("et-123"),
            version: "1.0".to_string(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("eventTypeId"));
    }
}
