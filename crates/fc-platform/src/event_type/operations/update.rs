//! Update Event Type Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::EventTypeUpdated;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::EventType;
use crate::EventTypeRepository;
use crate::EventTypeStatus;

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

impl crate::usecase::AuditMasked for UpdateEventTypeCommand {}

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
}

#[async_trait]
impl<U: UnitOfWork> UseCase for UpdateEventTypeUseCase<U> {
    type Command = UpdateEventTypeCommand;
    type Event = EventTypeUpdated;

    async fn validate(&self, command: &UpdateEventTypeCommand) -> Result<(), UseCaseError> {
        if command.event_type_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "EVENT_TYPE_ID_REQUIRED",
                "Event type ID is required",
            ));
        }

        if command.name.is_none() && command.description.is_none() {
            return Err(UseCaseError::validation(
                "NO_UPDATES",
                "At least one field must be provided for update",
            ));
        }

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &UpdateEventTypeCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: UpdateEventTypeCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<EventTypeUpdated> {
        let (event_type, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        // Atomic commit
        self.unit_of_work
            .commit(&event_type, &*self.event_type_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> UpdateEventTypeUseCase<U> {
    async fn prepare(
        &self,
        command: &UpdateEventTypeCommand,
        ctx: &ExecutionContext,
    ) -> Result<(EventType, EventTypeUpdated), UseCaseError> {
        // Fetch existing event type
        let mut event_type = self
            .event_type_repo
            .find_by_id(&command.event_type_id)
            .await
            .or_not_found(
                "EVENT_TYPE_NOT_FOUND",
                format!("Event type with ID '{}' not found", command.event_type_id),
            )?;

        // Business rule: can only update active event types
        if event_type.status == EventTypeStatus::Archived {
            return Err(UseCaseError::business_rule(
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
            return Err(UseCaseError::validation(
                "NO_CHANGES",
                "No changes detected",
            ));
        }

        event_type.updated_at = chrono::Utc::now();

        // Create domain event
        let event = EventTypeUpdated::new(ctx, &event_type.id, updated_name, updated_description);
        Ok((event_type, event))
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
