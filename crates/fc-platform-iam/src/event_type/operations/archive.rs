//! Archive Event Type Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::EventTypeArchived;
use crate::event_type::entity::EventTypeStatus;
use crate::event_type::repository::EventTypeRepository;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for archiving an event type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveEventTypeCommand {
    /// Event type ID to archive
    pub event_type_id: String,
}

impl fc_platform_core::usecase::AuditMasked for ArchiveEventTypeCommand {}

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

    /// Go `CheckScopeAccess` on the stored event type (Go checks it post-load):
    /// a client's type needs that client, a platform one anchor scope (403
    /// `SCOPE_FORBIDDEN`). A missing type is `execute`'s 404. Holds for the
    /// `/api`, `/bff` and fc-web routes alike.
    async fn authorize(
        &self,
        command: &ArchiveEventTypeCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        if let Some(event_type) = self
            .event_type_repo
            .find_by_id(&command.event_type_id)
            .await?
        {
            fc_platform_core::shared::caller_reach::check_scope_access(
                ctx.caller(),
                event_type.client_id.as_deref(),
            )?;
        }
        Ok(())
    }

    async fn execute(
        &self,
        command: ArchiveEventTypeCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<EventTypeArchived>, UseCaseError> {
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
        let event = EventTypeArchived::new(&ctx, &event_type.id, &event_type.code);

        // Atomic commit
        self.unit_of_work
            .commit(&event_type, &*self.event_type_repo, event, &command)
            .await
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
