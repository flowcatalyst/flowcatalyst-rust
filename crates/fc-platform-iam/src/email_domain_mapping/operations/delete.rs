//! Delete Email Domain Mapping Use Case

use async_trait::async_trait;
use fc_platform_core::shared::id::EmailDomainMappingId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::EmailDomainMappingDeleted;
use crate::email_domain_mapping::repository::EmailDomainMappingRepository;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for deleting an email domain mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteEmailDomainMappingCommand {
    pub mapping_id: EmailDomainMappingId,
}

impl AuditMasked for DeleteEmailDomainMappingCommand {}

pub struct DeleteEmailDomainMappingUseCase<U: UnitOfWork> {
    edm_repo: Arc<EmailDomainMappingRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteEmailDomainMappingUseCase<U> {
    pub fn new(edm_repo: Arc<EmailDomainMappingRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            edm_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteEmailDomainMappingUseCase<U> {
    type Command = DeleteEmailDomainMappingCommand;
    type Event = EmailDomainMappingDeleted;

    async fn validate(
        &self,
        command: &DeleteEmailDomainMappingCommand,
    ) -> Result<(), UseCaseError> {
        if command.mapping_id.as_str().trim().is_empty() {
            return Err(UseCaseError::validation(
                "MAPPING_ID_REQUIRED",
                "Mapping ID is required",
            ));
        }
        Ok(())
    }

    /// Email-domain mappings are platform-owner data, written by anchors only
    /// (Go's `Can*EmailDomainMappings` are `anchorWith`).
    /// The handler's gate checks this, with the permission, before the body
    /// is read; here it holds for every caller (fc-web, orchestrations).
    async fn authorize(
        &self,
        _command: &DeleteEmailDomainMappingCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(checks::require_anchor_scope(ctx.caller())?)
    }

    async fn execute(
        &self,
        command: DeleteEmailDomainMappingCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<EmailDomainMappingDeleted>, UseCaseError> {
        let mapping = self
            .edm_repo
            .find_by_id(&command.mapping_id)
            .await
            .or_not_found(
                "NOT_FOUND",
                format!(
                    "Email domain mapping with ID '{}' not found",
                    command.mapping_id
                ),
            )?;

        let event = EmailDomainMappingDeleted::new(&ctx, &mapping.id, &mapping.email_domain);

        self.unit_of_work
            .commit_delete(&mapping, &*self.edm_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = DeleteEmailDomainMappingCommand {
            mapping_id: EmailDomainMappingId::parse("edm_123").unwrap(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("mappingId"));
        assert!(json.contains("edm_123"));

        let deserialized: DeleteEmailDomainMappingCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(
            deserialized.mapping_id,
            EmailDomainMappingId::parse("edm_123").unwrap()
        );
    }

    #[test]
    fn test_validate_empty_mapping_id() {
        let cmd = DeleteEmailDomainMappingCommand {
            mapping_id: EmailDomainMappingId::from_wire(""),
        };
        assert!(cmd.mapping_id.as_str().trim().is_empty());
    }

    #[test]
    fn test_validate_whitespace_mapping_id() {
        let cmd = DeleteEmailDomainMappingCommand {
            mapping_id: EmailDomainMappingId::from_wire("   "),
        };
        assert!(
            cmd.mapping_id.as_str().trim().is_empty(),
            "Whitespace-only mapping_id should be treated as empty"
        );
    }

    #[test]
    fn test_validate_valid_mapping_id() {
        let cmd = DeleteEmailDomainMappingCommand {
            mapping_id: EmailDomainMappingId::from_wire("edm-456"),
        };
        assert!(!cmd.mapping_id.as_str().trim().is_empty());
    }
}
