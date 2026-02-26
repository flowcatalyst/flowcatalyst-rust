//! Archive Event Type Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::EventTypeStatus;
use crate::EventTypeRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::EventTypeArchived;

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

    pub async fn execute(
        &self,
        command: ArchiveEventTypeCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<EventTypeArchived> {
        // Validation: event_type_id is required
        if command.event_type_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "EVENT_TYPE_ID_REQUIRED",
                "Event type ID is required",
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

        // Business rule: can only archive active or draft event types
        if event_type.status == EventTypeStatus::Archive {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "ALREADY_ARCHIVED",
                "Event type is already archived",
            ));
        }

        // Archive the event type
        event_type.archive();

        // Create domain event
        let event = EventTypeArchived::new(&ctx, &event_type.id, &event_type.code);

        // Atomic commit
        self.unit_of_work.commit(&event_type, event, &command).await
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
