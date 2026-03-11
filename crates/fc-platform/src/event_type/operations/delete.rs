//! Delete Event Type Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::event_type::entity::SpecVersionStatus;
use crate::EventTypeStatus;
use crate::EventTypeRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::EventTypeDeleted;

/// Command for deleting an event type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteEventTypeCommand {
    /// Event type ID to delete
    pub event_type_id: String,
}

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
        Self { event_type_repo, unit_of_work }
    }

    pub async fn execute(
        &self,
        command: DeleteEventTypeCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<EventTypeDeleted> {
        if command.event_type_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "EVENT_TYPE_ID_REQUIRED",
                "Event type ID is required",
            ));
        }

        let event_type = match self.event_type_repo.find_by_id(&command.event_type_id).await {
            Ok(Some(et)) => et,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "EVENT_TYPE_NOT_FOUND",
                    format!("Event type with ID '{}' not found", command.event_type_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch event type: {}", e
                )));
            }
        };

        // Business rule: can only delete if ARCHIVED or all versions are FINALISING
        let all_finalising = event_type.spec_versions.iter()
            .all(|sv| sv.status == SpecVersionStatus::Finalising);

        if event_type.status != EventTypeStatus::Archived && !all_finalising {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "CANNOT_DELETE",
                "Can only delete archived event types or those with all versions in FINALISING status",
            ));
        }

        let event = EventTypeDeleted::new(&ctx, &event_type.id, &event_type.code);

        self.unit_of_work.commit_delete(&event_type, event, &command).await
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
