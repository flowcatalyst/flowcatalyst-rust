//! Delete Event Type Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::EventTypeDeleted;
use crate::event_type::entity::EventTypeStatus;
use crate::event_type::entity::SpecVersionStatus;
use crate::event_type::repository::EventTypeRepository;
use fc_platform_core::shared::caller_reach;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for deleting an event type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteEventTypeCommand {
    /// Event type ID to delete
    pub event_type_id: String,
}

impl AuditMasked for DeleteEventTypeCommand {}

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

    /// Go `CheckScopeAccess` on the stored event type (Go checks it post-load):
    /// a client's type needs that client, a platform one anchor scope (403
    /// `SCOPE_FORBIDDEN`). A missing type is `execute`'s 404. Holds for the
    /// `/api`, `/bff` and fc-web routes alike.
    async fn authorize(
        &self,
        command: &DeleteEventTypeCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        if let Some(event_type) = self
            .event_type_repo
            .find_by_id(&command.event_type_id)
            .await?
        {
            caller_reach::check_scope_access(ctx.caller(), event_type.client_id.as_deref())?;
        }
        Ok(())
    }

    async fn execute(
        &self,
        command: DeleteEventTypeCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<EventTypeDeleted>, UseCaseError> {
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

        let event = EventTypeDeleted::new(&ctx, &event_type.id, &event_type.code);

        self.unit_of_work
            .commit_delete(&event_type, &*self.event_type_repo, event, &command)
            .await
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
