//! Update Event Type Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::EventTypeStatus;
use crate::EventTypeRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::EventTypeUpdated;

/// Command for updating an existing event type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEventTypeCommand {
    /// Event type ID to update
    pub event_type_id: String,

    /// New name (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// New description (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Use case for updating an existing event type.
pub struct UpdateEventTypeUseCase<U: UnitOfWork> {
    event_type_repo: Arc<EventTypeRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateEventTypeUseCase<U> {
    pub fn new(event_type_repo: Arc<EventTypeRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            event_type_repo,
            unit_of_work,
        }
    }

    pub async fn execute(
        &self,
        command: UpdateEventTypeCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<EventTypeUpdated> {
        // Validation: event_type_id is required
        if command.event_type_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "EVENT_TYPE_ID_REQUIRED",
                "Event type ID is required",
            ));
        }

        // Validation: at least one field to update
        if command.name.is_none() && command.description.is_none() {
            return UseCaseResult::failure(UseCaseError::validation(
                "NO_UPDATES",
                "At least one field must be provided for update",
            ));
        }

        // Fetch existing event type
        let mut event_type = match self.event_type_repo.find_by_id(&command.event_type_id).await {
            Ok(Some(et)) => et,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "EVENT_TYPE_NOT_FOUND",
                    format!("Event type with ID '{}' not found", command.event_type_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch event type: {}",
                    e
                )));
            }
        };

        // Business rule: can only update active event types
        if event_type.status == EventTypeStatus::Archive {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "CANNOT_UPDATE_ARCHIVED",
                "Cannot update an archived event type",
            ));
        }

        // Track changes
        let mut updated_name: Option<&str> = None;
        let mut updated_description: Option<&str> = None;

        // Apply updates
        if let Some(ref name) = command.name {
            let name = name.trim();
            if name != event_type.name {
                event_type.name = name.to_string();
                updated_name = Some(name);
            }
        }

        if let Some(ref desc) = command.description {
            let changed = event_type.description.as_deref() != Some(desc.as_str());
            if changed {
                event_type.description = Some(desc.clone());
                updated_description = Some(desc.as_str());
            }
        }

        // Check if anything actually changed
        if updated_name.is_none() && updated_description.is_none() {
            return UseCaseResult::failure(UseCaseError::validation(
                "NO_CHANGES",
                "No changes detected",
            ));
        }

        event_type.updated_at = chrono::Utc::now();

        // Create domain event
        let event = EventTypeUpdated::new(
            &ctx,
            &event_type.id,
            updated_name,
            updated_description,
        );

        // Atomic commit
        self.unit_of_work.commit(&event_type, event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = UpdateEventTypeCommand {
            event_type_id: "et-123".to_string(),
            name: Some("New Name".to_string()),
            description: Some("New Description".to_string()),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("eventTypeId"));
        assert!(json.contains("New Name"));
    }
}
