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

        // Apply updates. Go's `UpdateEventType` saves and emits even when
        // nothing changed: a repeated update is a 204, not an error.
        if let Some(ref name) = command.name {
            event_type.name = name.trim().to_string();
        }
        if let Some(ref desc) = command.description {
            event_type.description = Some(desc.clone());
        }
        event_type.updated_at = chrono::Utc::now();

        // Create domain event
        let event = EventTypeUpdated::new(
            ctx,
            &event_type.id,
            &event_type.name,
            event_type.description.as_deref(),
        );
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
