//! Archive Event Type Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::EventTypeArchived;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::EventType;
use crate::EventTypeRepository;
use crate::EventTypeStatus;

/// Command for archiving an event type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveEventTypeCommand {
    /// Event type ID to archive
    pub event_type_id: String,
}

/// Use case for archiving an event type.
pub struct ArchiveEventTypeUseCase<U: UnitOfWork> {
    event_type_repo: Arc<EventTypeRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> ArchiveEventTypeUseCase<U> {
    pub fn new(event_type_repo: Arc<EventTypeRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            event_type_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for ArchiveEventTypeUseCase<U> {
    type Command = ArchiveEventTypeCommand;
    type Event = EventTypeArchived;

    async fn validate(&self, command: &ArchiveEventTypeCommand) -> Result<(), UseCaseError> {
        if command.event_type_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "EVENT_TYPE_ID_REQUIRED",
                "Event type ID is required",
            ));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &ArchiveEventTypeCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: ArchiveEventTypeCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<EventTypeArchived> {
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

impl<U: UnitOfWork> ArchiveEventTypeUseCase<U> {
    async fn prepare(
        &self,
        command: &ArchiveEventTypeCommand,
        ctx: &ExecutionContext,
    ) -> Result<(EventType, EventTypeArchived), UseCaseError> {
        // Fetch existing event type
        let mut event_type = self
            .event_type_repo
            .find_by_id(&command.event_type_id)
            .await
            .or_not_found(
                "EVENT_TYPE_NOT_FOUND",
                format!("Event type with ID '{}' not found", command.event_type_id),
            )?;

        // Business rule: can only archive active or draft event types
        if event_type.status == EventTypeStatus::Archived {
            return Err(UseCaseError::business_rule(
                "ALREADY_ARCHIVED",
                "Event type is already archived",
            ));
        }

        // Archive the event type
        event_type.archive();

        // Create domain event
        let event = EventTypeArchived::new(ctx, &event_type.id, &event_type.code);
        Ok((event_type, event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = ArchiveEventTypeCommand {
            event_type_id: "et-123".to_string(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("eventTypeId"));
    }
}
