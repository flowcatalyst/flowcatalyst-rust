//! Create Event Type Use Case
//!
//! Use case for creating a new event type.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::EventTypeCreated;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};
use crate::EventType;
use crate::EventTypeRepository;

/// Command for creating a new event type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEventTypeCommand {
    /// Event type code following format: {application}:{subdomain}:{aggregate}:{event}
    pub code: String,

    /// Human-readable name
    pub name: String,

    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Optional client ID for multi-tenant scoping
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Optional initial schema payload. When provided, persisted as spec
    /// version `1.0`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,
}

impl crate::usecase::AuditMasked for CreateEventTypeCommand {}

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
/// let command = CreateEventTypeCommand {
///     code: "orders:fulfillment:shipment:shipped".to_string(),
///     name: "Shipment Shipped".to_string(),
///     description: Some("Emitted when a shipment leaves".to_string()),
///     client_id: None,
/// };
///
/// let result = use_case.execute(command, ctx).await;
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

    async fn validate(&self, command: &CreateEventTypeCommand) -> Result<(), UseCaseError> {
        // Validation: code is required
        if command.code.trim().is_empty() {
            return Err(UseCaseError::validation(
                "CODE_REQUIRED",
                "Event type code is required",
            ));
        }

        // Validation: name is required
        if command.name.trim().is_empty() {
            return Err(UseCaseError::validation(
                "NAME_REQUIRED",
                "Event type name is required",
            ));
        }

        // Validation: code format
        let parts: Vec<&str> = command.code.split(':').collect();
        if parts.len() != 4 {
            return Err(UseCaseError::validation(
                "INVALID_CODE_FORMAT",
                "Event type code must follow format: application:subdomain:aggregate:event",
            ));
        }

        // Validate each part is not empty
        for (i, part) in parts.iter().enumerate() {
            if part.trim().is_empty() {
                let part_name = match i {
                    0 => "application",
                    1 => "subdomain",
                    2 => "aggregate",
                    3 => "event",
                    _ => "unknown",
                };
                return Err(UseCaseError::validation(
                    "INVALID_CODE_FORMAT",
                    format!("Event type code part '{}' cannot be empty", part_name),
                ));
            }
        }

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &CreateEventTypeCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: CreateEventTypeCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<EventTypeCreated> {
        // Business rule: code must be unique
        let existing = match self.event_type_repo.find_by_code(&command.code).await {
            Ok(found) => found,
            Err(e) => return UseCaseResult::failure(e.into()),
        };
        if existing.is_some() {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "CODE_EXISTS",
                format!("Event type with code '{}' already exists", command.code),
            ));
        }

        // Create the event type entity
        let event_type = match EventType::new(&command.code, &command.name) {
            Ok(mut et) => {
                if let Some(desc) = &command.description {
                    et.description = Some(desc.clone());
                }
                if let Some(client_id) = &command.client_id {
                    et.client_id = Some(client_id.clone());
                }
                if let Some(schema) = &command.schema {
                    let spec = crate::SpecVersion::new(&et.id, "1.0", Some(schema.clone()));
                    et.add_schema_version(spec);
                }
                et.created_by = Some(ctx.principal_id.clone());
                et
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::validation(
                    "INVALID_CODE_FORMAT",
                    e.to_string(),
                ));
            }
        };

        let event = EventTypeCreated {
            metadata: EventTypeCreated::metadata_for(&ctx, &event_type.id),
            event_type_id: event_type.id.clone(),
            code: event_type.code.clone(),
            name: event_type.name.clone(),
            description: command.description.clone(),
            application: event_type.application.clone(),
            subdomain: event_type.subdomain.clone(),
            aggregate: event_type.aggregate.clone(),
            event_name: event_type.event_name.clone(),
            client_id: command.client_id.clone(),
        };

        // Atomic commit: entity + event + audit log
        self.unit_of_work
            .commit(&event_type, &*self.event_type_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::unit_of_work::HasId;

    // Helper to create a mock repository
    // For now, we'll just test the validation logic

    #[test]
    fn test_command_serialization() {
        let cmd = CreateEventTypeCommand {
            code: "orders:fulfillment:shipment:shipped".to_string(),
            name: "Shipment Shipped".to_string(),
            description: Some("When a shipment leaves".to_string()),
            client_id: None,
            schema: None,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("orders:fulfillment:shipment:shipped"));
    }

    #[test]
    fn test_event_type_has_id() {
        let et = EventType::new("app:domain:agg:evt", "Test Event").unwrap();
        assert!(!et.id().is_empty());
    }
}
