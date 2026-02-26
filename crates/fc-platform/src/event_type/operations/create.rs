//! Create Event Type Use Case
//!
//! Use case for creating a new event type.

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::EventType;
use crate::EventTypeRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
    unit_of_work::HasId,
};
use super::events::EventTypeCreated;

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
}

/// Implement HasId for EventType to work with UnitOfWork
impl HasId for EventType {
    fn id(&self) -> &str {
        &self.id
    }

    fn collection_name() -> &'static str {
        "event_types"
    }
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
    pub fn new(
        event_type_repo: Arc<EventTypeRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            event_type_repo,
            unit_of_work,
        }
    }

    /// Execute the use case.
    ///
    /// # Validation
    /// - Code must follow format: {application}:{subdomain}:{aggregate}:{event}
    /// - Code must be unique
    /// - Name is required
    ///
    /// # Returns
    /// - `UseCaseResult::Success(EventTypeCreated)` on success
    /// - `UseCaseResult::Failure(UseCaseError)` on validation or business rule violation
    pub async fn execute(
        &self,
        command: CreateEventTypeCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<EventTypeCreated> {
        // Validation: code is required
        if command.code.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "CODE_REQUIRED",
                "Event type code is required",
            ));
        }

        // Validation: name is required
        if command.name.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "NAME_REQUIRED",
                "Event type name is required",
            ));
        }

        // Validation: code format
        let parts: Vec<&str> = command.code.split(':').collect();
        if parts.len() != 4 {
            return UseCaseResult::failure(UseCaseError::validation(
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
                return UseCaseResult::failure(UseCaseError::validation(
                    "INVALID_CODE_FORMAT",
                    format!("Event type code part '{}' cannot be empty", part_name),
                ));
            }
        }

        // Business rule: code must be unique
        if let Ok(Some(_)) = self.event_type_repo.find_by_code(&command.code).await {
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
                et.created_by = Some(ctx.principal_id.clone());
                et
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::validation(
                    "INVALID_CODE_FORMAT",
                    e,
                ));
            }
        };

        // Create domain event
        let event = EventTypeCreated::builder()
            .from_context(&ctx)
            .event_type_id(&event_type.id)
            .code(&event_type.code)
            .name(&event_type.name)
            .application(&event_type.application)
            .subdomain(&event_type.subdomain)
            .aggregate(&event_type.aggregate)
            .event_name(&event_type.event_name)
            .build();

        // Add optional fields
        let event = if let Some(desc) = &command.description {
            EventTypeCreated {
                description: Some(desc.clone()),
                ..event
            }
        } else {
            event
        };

        let event = if let Some(client_id) = &command.client_id {
            EventTypeCreated {
                client_id: Some(client_id.clone()),
                ..event
            }
        } else {
            event
        };

        // Atomic commit: entity + event + audit log
        self.unit_of_work.commit(&event_type, event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    
    

    // Helper to create a mock repository
    // Note: In a real test, you would use a mock or in-memory MongoDB
    // For now, we'll just test the validation logic

    #[test]
    fn test_command_serialization() {
        let cmd = CreateEventTypeCommand {
            code: "orders:fulfillment:shipment:shipped".to_string(),
            name: "Shipment Shipped".to_string(),
            description: Some("When a shipment leaves".to_string()),
            client_id: None,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("orders:fulfillment:shipment:shipped"));
    }

    #[test]
    fn test_event_type_has_id() {
        let et = EventType::new("app:domain:agg:evt", "Test Event").unwrap();
        assert!(!et.id().is_empty());
        assert_eq!(EventType::collection_name(), "event_types");
    }
}
