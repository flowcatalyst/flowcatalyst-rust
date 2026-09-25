//! Add Schema (Spec Version) Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::SchemaAdded;
use crate::event_type::entity::{SchemaType, SpecVersion};
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::EventType;
use crate::EventTypeRepository;

/// Command for adding a new schema version to an event type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddSchemaCommand {
    /// Event type ID
    pub event_type_id: String,

    /// Schema version (MAJOR.MINOR format, e.g. "1.0")
    pub version: String,

    /// MIME type (e.g. "application/schema+json")
    #[serde(default = "default_mime_type")]
    pub mime_type: String,

    /// Schema content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_content: Option<serde_json::Value>,

    /// Schema type
    #[serde(default)]
    pub schema_type: Option<SchemaType>,
}

impl crate::usecase::AuditMasked for AddSchemaCommand {}

fn default_mime_type() -> String {
    "application/schema+json".to_string()
}

/// Use case for adding a new spec version to an event type.
pub struct AddSchemaUseCase<U: UnitOfWork> {
    event_type_repo: Arc<EventTypeRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> AddSchemaUseCase<U> {
    pub fn new(event_type_repo: Arc<EventTypeRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            event_type_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for AddSchemaUseCase<U> {
    type Command = AddSchemaCommand;
    type Event = SchemaAdded;

    async fn validate(&self, command: &AddSchemaCommand) -> Result<(), UseCaseError> {
        if command.event_type_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "EVENT_TYPE_ID_REQUIRED",
                "Event type ID is required",
            ));
        }

        let version = command.version.trim();
        if version.is_empty() {
            return Err(UseCaseError::validation(
                "VERSION_REQUIRED",
                "Schema version is required",
            ));
        }
        let parts: Vec<&str> = version.split('.').collect();
        if parts.len() != 2 || parts.iter().any(|p| p.parse::<u32>().is_err()) {
            return Err(UseCaseError::validation(
                "INVALID_VERSION_FORMAT",
                "Version must be in MAJOR.MINOR format (e.g. 1.0)",
            ));
        }

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &AddSchemaCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: AddSchemaCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<SchemaAdded> {
        let (event_type, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit(&event_type, &*self.event_type_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> AddSchemaUseCase<U> {
    async fn prepare(
        &self,
        command: &AddSchemaCommand,
        ctx: &ExecutionContext,
    ) -> Result<(EventType, SchemaAdded), UseCaseError> {
        let version = command.version.trim();

        // Fetch event type
        let mut event_type = self
            .event_type_repo
            .find_by_id(&command.event_type_id)
            .await
            .or_not_found(
                "EVENT_TYPE_NOT_FOUND",
                format!("Event type with ID '{}' not found", command.event_type_id),
            )?;

        // Business rule: cannot add schema to archived event type
        if event_type.status == crate::EventTypeStatus::Archived {
            return Err(UseCaseError::business_rule(
                "EVENT_TYPE_ARCHIVED",
                "Cannot add schema to an archived event type",
            ));
        }

        // Business rule: version must not already exist
        if event_type
            .spec_versions
            .iter()
            .any(|sv| sv.version == version)
        {
            return Err(UseCaseError::business_rule(
                "VERSION_EXISTS",
                format!("Schema version '{}' already exists", version),
            ));
        }

        // Create spec version
        let mut spec_version =
            SpecVersion::new(&event_type.id, version, command.schema_content.clone());
        spec_version.mime_type = command.mime_type.clone();
        if let Some(st) = command.schema_type {
            spec_version.schema_type = st;
        }

        // Add to event type
        event_type.add_schema_version(spec_version);

        // Create domain event
        let event = SchemaAdded::new(ctx, &event_type.id, version);
        Ok((event_type, event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = AddSchemaCommand {
            event_type_id: "et-123".to_string(),
            version: "1.0".to_string(),
            mime_type: "application/schema+json".to_string(),
            schema_content: Some(serde_json::json!({"type": "object"})),
            schema_type: None,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("eventTypeId"));
        assert!(json.contains("1.0"));
    }
}
