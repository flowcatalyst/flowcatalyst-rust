//! Add Schema (Spec Version) Use Case

use async_trait::async_trait;
use fc_platform_core::shared::id::EventTypeId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::SchemaAdded;
use crate::event_type::entity::EventTypeStatus;
use crate::event_type::entity::{SchemaType, SpecVersion};
use crate::event_type::repository::EventTypeRepository;
use fc_platform_core::shared::caller_reach;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for adding a new schema version to an event type.
///
/// `schema_type` (JSON_SCHEMA, XSD, PROTO; the legacy XML_SCHEMA and PROTOBUF
/// accepted, any case) and `mime_type` are optional: absent or blank means
/// JSON_SCHEMA and the schema type's default MIME type (Go `AddSchemaCommand`,
/// bug 3). A JSON Schema is stored as the JSON document; a schema sent as a
/// JSON string holding the document (the SPA sends its textarea's text this
/// way) is stored as the document it holds. XSD and PROTO schemas are text,
/// stored as a JSON string.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddSchemaCommand {
    /// Event type ID
    pub event_type_id: EventTypeId,

    /// Schema version (MAJOR.MINOR format, e.g. "1.0")
    pub version: String,

    /// MIME type (e.g. "application/schema+json"); absent or blank: the
    /// schema type's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,

    /// Schema content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_content: Option<serde_json::Value>,

    /// Schema type as sent; absent or blank: JSON_SCHEMA.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_type: Option<String>,
}

impl AuditMasked for AddSchemaCommand {}

/// `msg_event_type_spec_versions.mime_type`'s width.
const MAX_MIME_TYPE_LEN: usize = 100;

/// The command's schema type: absent or blank is JSON_SCHEMA, anything
/// unknown 400 `INVALID_SCHEMA_TYPE` (Go `schemaTypeOf`).
fn schema_type_of(raw: Option<&str>) -> Result<SchemaType, UseCaseError> {
    match raw.map(str::trim).filter(|v| !v.is_empty()) {
        None => Ok(SchemaType::JsonSchema),
        Some(v) => v.to_uppercase().parse().map_err(|_| {
            UseCaseError::validation(
                "INVALID_SCHEMA_TYPE",
                "schemaType must be JSON_SCHEMA, XSD or PROTO",
            )
        }),
    }
}

/// The stored schema content, or 400 `INVALID_SCHEMA` (Go
/// `NormaliseSchemaContent`).
fn normalised_content(
    schema_type: SchemaType,
    content: &Option<serde_json::Value>,
) -> Result<serde_json::Value, UseCaseError> {
    let content = content
        .clone()
        .ok_or_else(|| UseCaseError::validation("SCHEMA_REQUIRED", "schema payload is required"))?;
    schema_type
        .normalise_content(content)
        .map_err(|message| UseCaseError::validation("INVALID_SCHEMA", message))
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
        if command.event_type_id.as_str().trim().is_empty() {
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
        if command
            .mime_type
            .as_deref()
            .is_some_and(|m| m.trim().len() > MAX_MIME_TYPE_LEN)
        {
            return Err(UseCaseError::validation(
                "INVALID_MIME_TYPE",
                "mimeType is longer than 100 characters",
            ));
        }
        let schema_type = schema_type_of(command.schema_type.as_deref())?;
        normalised_content(schema_type, &command.schema_content)?;

        Ok(())
    }

    /// Go `CheckScopeAccess` on the stored event type (Go checks it post-load):
    /// a client's type needs that client, a platform one anchor scope (403
    /// `SCOPE_FORBIDDEN`). A missing type is `execute`'s 404. Holds for the
    /// `/api`, `/bff` and fc-web routes alike.
    async fn authorize(
        &self,
        command: &AddSchemaCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        if let Some(event_type) = self
            .event_type_repo
            .find_by_id(&command.event_type_id)
            .await?
        {
            caller_reach::check_scope_access(ctx.caller(), event_type.client_id.as_ref())?;
        }
        Ok(())
    }

    async fn execute(
        &self,
        command: AddSchemaCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<SchemaAdded>, UseCaseError> {
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
        if event_type.status == EventTypeStatus::Archived {
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
                format!(
                    "Schema version '{}' already exists for this event type",
                    version
                ),
            ));
        }

        // Create spec version: the normalised content, the schema type, and
        // the MIME type sent or the type's default.
        let schema_type = schema_type_of(command.schema_type.as_deref())?;
        let content = normalised_content(schema_type, &command.schema_content)?;
        let mut spec_version = SpecVersion::new(event_type.id.clone(), version, Some(content));
        spec_version.schema_type = schema_type;
        spec_version.mime_type = command
            .mime_type
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or(schema_type.default_mime_type())
            .to_string();

        // Add to event type
        event_type.add_schema_version(spec_version);

        // Create domain event
        let event = SchemaAdded::new(&ctx, &event_type.id, version);

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
        let cmd = AddSchemaCommand {
            event_type_id: EventTypeId::from_wire("et-123"),
            version: "1.0".to_string(),
            mime_type: None,
            schema_content: Some(serde_json::json!({"type": "object"})),
            schema_type: None,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("eventTypeId"));
        assert!(json.contains("1.0"));
    }
}
