//! Delete Email Domain Mapping Use Case

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::EmailDomainMappingRepository;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};
use super::events::EmailDomainMappingDeleted;

/// Command for deleting an email domain mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteEmailDomainMappingCommand {
    pub mapping_id: String,
}

pub struct DeleteEmailDomainMappingUseCase<U: UnitOfWork> {
    edm_repo: Arc<EmailDomainMappingRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteEmailDomainMappingUseCase<U> {
    pub fn new(edm_repo: Arc<EmailDomainMappingRepository>, unit_of_work: Arc<U>) -> Self {
        Self { edm_repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteEmailDomainMappingUseCase<U> {
    type Command = DeleteEmailDomainMappingCommand;
    type Event = EmailDomainMappingDeleted;

    async fn validate(&self, command: &DeleteEmailDomainMappingCommand) -> Result<(), UseCaseError> {
        if command.mapping_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "MAPPING_ID_REQUIRED", "Mapping ID is required",
            ));
        }
        Ok(())
    }

    async fn authorize(&self, _command: &DeleteEmailDomainMappingCommand, _ctx: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: DeleteEmailDomainMappingCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<EmailDomainMappingDeleted> {
        let mapping = match self.edm_repo.find_by_id(&command.mapping_id).await {
            Ok(Some(m)) => m,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "NOT_FOUND",
                    format!("Email domain mapping with ID '{}' not found", command.mapping_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch email domain mapping: {}", e
                )));
            }
        };

        if let Err(e) = self.edm_repo.delete(&mapping.id).await {
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to delete email domain mapping: {}", e
            )));
        }

        let event = EmailDomainMappingDeleted::new(
            &ctx,
            &mapping.id,
            &mapping.email_domain,
        );

        self.unit_of_work.emit_event(event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = DeleteEmailDomainMappingCommand {
            mapping_id: "edm-123".to_string(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("mappingId"));
        assert!(json.contains("edm-123"));

        let deserialized: DeleteEmailDomainMappingCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.mapping_id, "edm-123");
    }

    #[test]
    fn test_validate_empty_mapping_id() {
        let cmd = DeleteEmailDomainMappingCommand {
            mapping_id: "".to_string(),
        };
        assert!(cmd.mapping_id.trim().is_empty());
    }

    #[test]
    fn test_validate_whitespace_mapping_id() {
        let cmd = DeleteEmailDomainMappingCommand {
            mapping_id: "   ".to_string(),
        };
        assert!(cmd.mapping_id.trim().is_empty(), "Whitespace-only mapping_id should be treated as empty");
    }

    #[test]
    fn test_validate_valid_mapping_id() {
        let cmd = DeleteEmailDomainMappingCommand {
            mapping_id: "edm-456".to_string(),
        };
        assert!(!cmd.mapping_id.trim().is_empty());
    }
}
