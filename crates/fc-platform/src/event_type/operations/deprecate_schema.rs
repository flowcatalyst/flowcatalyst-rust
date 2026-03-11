//! Deprecate Schema Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::event_type::entity::SpecVersionStatus;
use crate::EventTypeRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::SchemaDeprecated;

/// Command for deprecating a schema version.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeprecateSchemaCommand {
    /// Event type ID
    pub event_type_id: String,

    /// Version to deprecate (e.g. "1.0")
    pub version: String,
}

/// Use case for deprecating a schema version (CURRENT → DEPRECATED).
pub struct DeprecateSchemaUseCase<U: UnitOfWork> {
    event_type_repo: Arc<EventTypeRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeprecateSchemaUseCase<U> {
    pub fn new(event_type_repo: Arc<EventTypeRepository>, unit_of_work: Arc<U>) -> Self {
        Self { event_type_repo, unit_of_work }
    }

    pub async fn execute(
        &self,
        command: DeprecateSchemaCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<SchemaDeprecated> {
        if command.event_type_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "EVENT_TYPE_ID_REQUIRED",
                "Event type ID is required",
            ));
        }
        if command.version.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "VERSION_REQUIRED",
                "Schema version is required",
            ));
        }

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
                    "Failed to fetch event type: {}", e
                )));
            }
        };

        let target_idx = event_type.spec_versions.iter().position(|sv| sv.version == command.version);
        let target_idx = match target_idx {
            Some(i) => i,
            None => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "VERSION_NOT_FOUND",
                    format!("Schema version '{}' not found", command.version),
                ));
            }
        };

        // Business rule: cannot deprecate FINALISING schemas
        if event_type.spec_versions[target_idx].status == SpecVersionStatus::Finalising {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "CANNOT_DEPRECATE_FINALISING",
                "Cannot deprecate a schema that is still in FINALISING status",
            ));
        }

        // Business rule: cannot deprecate already deprecated
        if event_type.spec_versions[target_idx].status == SpecVersionStatus::Deprecated {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "ALREADY_DEPRECATED",
                "Schema version is already deprecated",
            ));
        }

        // Deprecate
        event_type.spec_versions[target_idx].status = SpecVersionStatus::Deprecated;
        event_type.spec_versions[target_idx].updated_at = chrono::Utc::now();
        event_type.updated_at = chrono::Utc::now();

        let event = SchemaDeprecated::new(
            &ctx,
            &event_type.id,
            &command.version,
        );

        self.unit_of_work.commit(&event_type, event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = DeprecateSchemaCommand {
            event_type_id: "et-123".to_string(),
            version: "1.0".to_string(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("eventTypeId"));
    }
}
