//! Create Event Type Use Case
//!
//! Use case for creating a new event type.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::EventTypeCreated;
use crate::event_type::entity::EventType;
use crate::event_type::entity::SpecVersion;
use crate::event_type::entity::{EventTypeCode, EventTypeCodeError};
use crate::event_type::repository::EventTypeRepository;
use fc_platform_core::shared::caller_reach;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{Committed, ExecutionContext, UnitOfWork, UseCase, UseCaseError};

/// Command for creating a new event type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEventTypeCommand {
    /// Event type code following format: {application}:{subdomain}:{aggregate}:{event}.
    /// Parsed where the command is built ([`CreateEventTypeCommand::parse_code`]).
    pub code: EventTypeCode,

    /// Human-readable name
    pub name: String,

    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Optional client ID for multi-tenant scoping
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Events of this type are carried per client (Go `clientScoped`): the
    /// subscription editor offers client-scoped types only to client-scoped
    /// subscriptions. Distinct from `client_id`, which scopes the row itself.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub client_scoped: bool,

    /// Optional initial schema payload. When provided, persisted as spec
    /// version `1.0`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,
}

impl AuditMasked for CreateEventTypeCommand {}

impl CreateEventTypeCommand {
    /// Parse a requested code for this command, with Go `CreateEventType`'s
    /// validation errors in its order: `CODE_REQUIRED` for a blank code,
    /// then `NAME_REQUIRED` for a blank name, then `INVALID_CODE_FORMAT`.
    /// The name is looked at only so that a blank name still wins over a
    /// malformed code, as it did when all three were checked in `validate`.
    ///
    /// Handlers call this after their permission check, where they build
    /// the command, so an unauthorised caller gets 403 before any 400.
    pub fn parse_code(code: &str, name: &str) -> Result<EventTypeCode, UseCaseError> {
        let parsed = EventTypeCode::parse(code);
        if parsed == Err(EventTypeCodeError::Required) {
            return Err(UseCaseError::validation(
                "CODE_REQUIRED",
                "Event type code is required",
            ));
        }
        if name.trim().is_empty() {
            return Err(name_required());
        }
        parsed.map_err(|e| UseCaseError::validation("INVALID_CODE_FORMAT", e.to_string()))
    }
}

fn name_required() -> UseCaseError {
    UseCaseError::validation("NAME_REQUIRED", "Event type name is required")
}

/// Use case for creating a new event type.
///
/// # Example
///
/// ```ignore
/// let use_case = CreateEventTypeUseCase::new(
///     event_type_repo.clone(),
///     unit_of_work.clone(),
/// );
///
/// let name = "Shipment Shipped".to_string();
/// let command = CreateEventTypeCommand {
///     code: CreateEventTypeCommand::parse_code("orders:fulfillment:shipment:shipped", &name)?,
///     name,
///     description: Some("Emitted when a shipment leaves".to_string()),
///     client_id: None,
///     client_scoped: false,
///     schema: None,
/// };
///
/// let result = use_case.run(command, ctx).await;
/// ```
pub struct CreateEventTypeUseCase<U: UnitOfWork> {
    event_type_repo: Arc<EventTypeRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateEventTypeUseCase<U> {
    pub fn new(event_type_repo: Arc<EventTypeRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            event_type_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for CreateEventTypeUseCase<U> {
    type Command = CreateEventTypeCommand;
    type Event = EventTypeCreated;

    /// The code is an [`EventTypeCode`], parsed when the command was built.
    async fn validate(&self, command: &CreateEventTypeCommand) -> Result<(), UseCaseError> {
        if command.name.trim().is_empty() {
            return Err(name_required());
        }
        Ok(())
    }

    /// Go `CheckScopeAccess` on the requested client (Go's `CreateEventType`
    /// authorize): a client-scoped type needs that client, a platform one anchor
    /// scope (403 `SCOPE_FORBIDDEN`).
    async fn authorize(
        &self,
        command: &CreateEventTypeCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        caller_reach::check_scope_access(ctx.caller(), command.client_id.as_deref())
    }

    async fn execute(
        &self,
        command: CreateEventTypeCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<EventTypeCreated>, UseCaseError> {
        // Business rule: code must be unique
        let existing = self
            .event_type_repo
            .find_by_code(command.code.as_str())
            .await?;
        if existing.is_some() {
            return Err(UseCaseError::business_rule(
                "CODE_EXISTS",
                format!("Event type with code '{}' already exists", command.code),
            ));
        }

        // Create the event type entity
        let mut event_type = EventType::new(command.code.clone(), &command.name);
        if let Some(desc) = &command.description {
            event_type.description = Some(desc.clone());
        }
        if let Some(client_id) = &command.client_id {
            event_type.client_id = Some(client_id.clone());
        }
        event_type.client_scoped = command.client_scoped;
        if let Some(schema) = &command.schema {
            let spec = SpecVersion::new(&event_type.id, "1.0", Some(schema.clone()));
            event_type.add_schema_version(spec);
        }
        event_type.created_by = Some(ctx.principal_id.clone());

        let event = EventTypeCreated::new(&ctx, &event_type);

        // Atomic commit: entity + event + audit log
        self.unit_of_work
            .commit(&event_type, &*self.event_type_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_platform_core::usecase::unit_of_work::HasId;

    // Helper to create a mock repository
    // For now, we'll just test the validation logic

    #[test]
    fn test_command_serialization() {
        let cmd = CreateEventTypeCommand {
            code: EventTypeCode::parse("orders:fulfillment:shipment:shipped").unwrap(),
            name: "Shipment Shipped".to_string(),
            description: Some("When a shipment leaves".to_string()),
            client_id: None,
            client_scoped: false,
            schema: None,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""code":"orders:fulfillment:shipment:shipped""#));
        // Go's `clientScoped,omitempty`: absent unless set.
        assert!(!json.contains("clientScoped"));
        let scoped = CreateEventTypeCommand {
            client_scoped: true,
            ..cmd
        };
        let json = serde_json::to_string(&scoped).unwrap();
        assert!(json.contains(r#""clientScoped":true"#));
    }

    fn parse_err(code: &str, name: &str) -> (String, String) {
        let e = CreateEventTypeCommand::parse_code(code, name).unwrap_err();
        (e.code().to_string(), e.message().to_string())
    }

    /// The codes and messages `validate` answered before the code was
    /// parsed, in Go's order (code required, name required, format).
    #[test]
    fn parse_code_answers_the_validation_errors_in_gos_order() {
        let err = |code: &str, msg: &str| (code.to_string(), msg.to_string());
        assert_eq!(
            parse_err(" ", ""),
            err("CODE_REQUIRED", "Event type code is required")
        );
        assert_eq!(
            parse_err("a:b", " "),
            err("NAME_REQUIRED", "Event type name is required")
        );
        assert_eq!(
            parse_err("a:b:c", "X"),
            err(
                "INVALID_CODE_FORMAT",
                "Event type code must follow format: application:subdomain:aggregate:event"
            )
        );
        assert_eq!(
            parse_err("a: :c:d", "X"),
            err(
                "INVALID_CODE_FORMAT",
                "Event type code part 'subdomain' cannot be empty"
            )
        );
        assert_eq!(
            parse_err("a:b:c:", "X"),
            err(
                "INVALID_CODE_FORMAT",
                "Event type code part 'event' cannot be empty"
            )
        );
        assert_eq!(
            CreateEventTypeCommand::parse_code(" a:b:c:d ", "X")
                .unwrap()
                .as_str(),
            " a:b:c:d "
        );
    }

    #[test]
    fn test_event_type_has_id() {
        let et = EventType::new(
            EventTypeCode::parse("app:domain:agg:evt").unwrap(),
            "Test Event",
        );
        assert!(!et.id().is_empty());
    }
}
