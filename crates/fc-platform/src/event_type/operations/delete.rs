//! Delete Event Type Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::EventTypeDeleted;
use crate::event_type::entity::SpecVersionStatus;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::EventType;
use crate::EventTypeRepository;
use crate::EventTypeStatus;

/// Command for deleting an event type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteEventTypeCommand {
    /// Event type ID to delete
    pub event_type_id: String,
}

impl crate::usecase::AuditMasked for DeleteEventTypeCommand {}

/// Use case for deleting an event type.
///
/// Can only delete if:
/// - Status is ARCHIVED, OR
/// - Status is CURRENT with all spec versions in FINALISING status (never finalised)
pub struct DeleteEventTypeUseCase<U: UnitOfWork> {
    event_type_repo: Arc<EventTypeRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteEventTypeUseCase<U> {
    pub fn new(event_type_repo: Arc<EventTypeRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            event_type_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteEventTypeUseCase<U> {
    type Command = DeleteEventTypeCommand;
    type Event = EventTypeDeleted;

    async fn validate(&self, command: &DeleteEventTypeCommand) -> Result<(), UseCaseError> {
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
        _command: &DeleteEventTypeCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: DeleteEventTypeCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<EventTypeDeleted> {
        let (event_type, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit_delete(&event_type, &*self.event_type_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> DeleteEventTypeUseCase<U> {
    async fn prepare(
        &self,
        command: &DeleteEventTypeCommand,
        ctx: &ExecutionContext,
    ) -> Result<(EventType, EventTypeDeleted), UseCaseError> {
        let event_type = self
            .event_type_repo
            .find_by_id(&command.event_type_id)
            .await
            .or_not_found(
                "EVENT_TYPE_NOT_FOUND",
                format!("Event type with ID '{}' not found", command.event_type_id),
            )?;

        // Business rule: can only delete if ARCHIVED or all versions are FINALISING
        let all_finalising = event_type
            .spec_versions
            .iter()
            .all(|sv| sv.status == SpecVersionStatus::Finalising);

        if event_type.status != EventTypeStatus::Archived && !all_finalising {
            return Err(UseCaseError::business_rule(
                "CANNOT_DELETE",
                "Can only delete archived event types or those with all versions in FINALISING status",
            ));
        }

        let event = EventTypeDeleted::new(ctx, &event_type.id, &event_type.code);
        Ok((event_type, event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = DeleteEventTypeCommand {
            event_type_id: "et-123".to_string(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("eventTypeId"));
    }
}
