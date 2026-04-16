//! Update Email Domain Mapping Use Case

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::EmailDomainMappingRepository;
use crate::email_domain_mapping::entity::ScopeType;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};
use super::events::EmailDomainMappingUpdated;

/// Command for updating an email domain mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEmailDomainMappingCommand {
    pub mapping_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_roles_from_idp: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_client_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub granted_client_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_role_ids: Option<Vec<String>>,
}

pub struct UpdateEmailDomainMappingUseCase<U: UnitOfWork> {
    edm_repo: Arc<EmailDomainMappingRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateEmailDomainMappingUseCase<U> {
    pub fn new(edm_repo: Arc<EmailDomainMappingRepository>, unit_of_work: Arc<U>) -> Self {
        Self { edm_repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for UpdateEmailDomainMappingUseCase<U> {
    type Command = UpdateEmailDomainMappingCommand;
    type Event = EmailDomainMappingUpdated;

    async fn validate(&self, command: &UpdateEmailDomainMappingCommand) -> Result<(), UseCaseError> {
        if command.mapping_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "MAPPING_ID_REQUIRED", "Mapping ID is required",
            ));
        }
        Ok(())
    }

    async fn authorize(&self, _command: &UpdateEmailDomainMappingCommand, _ctx: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: UpdateEmailDomainMappingCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<EmailDomainMappingUpdated> {
        let mut mapping = match self.edm_repo.find_by_id(&command.mapping_id).await {
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

        // Selectively update fields
        if let Some(ref scope_type_str) = command.scope_type {
            mapping.scope_type = match scope_type_str.to_uppercase().as_str() {
                "ANCHOR" => ScopeType::Anchor,
                "PARTNER" => ScopeType::Partner,
                _ => ScopeType::Client,
            };
        }
        if let Some(ref primary_client_id) = command.primary_client_id {
            mapping.primary_client_id = Some(primary_client_id.clone());
        }
        if let Some(sync_roles) = command.sync_roles_from_idp {
            mapping.sync_roles_from_idp = sync_roles;
        }
        if let Some(ref additional) = command.additional_client_ids {
            mapping.additional_client_ids = additional.clone();
        }
        if let Some(ref granted) = command.granted_client_ids {
            mapping.granted_client_ids = granted.clone();
        }
        if let Some(ref roles) = command.allowed_role_ids {
            mapping.allowed_role_ids = roles.clone();
        }
        mapping.updated_at = chrono::Utc::now();

        if let Err(e) = self.edm_repo.update(&mapping).await {
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to update email domain mapping: {}", e
            )));
        }

        let event = EmailDomainMappingUpdated::new(
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
        let cmd = UpdateEmailDomainMappingCommand {
            mapping_id: "edm-123".to_string(),
            scope_type: Some("PARTNER".to_string()),
            primary_client_id: Some("client-456".to_string()),
            sync_roles_from_idp: Some(true),
            additional_client_ids: Some(vec!["c1".to_string(), "c2".to_string()]),
            granted_client_ids: Some(vec!["g1".to_string()]),
            allowed_role_ids: Some(vec!["r1".to_string(), "r2".to_string()]),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("mappingId"));
        assert!(json.contains("edm-123"));
        assert!(json.contains("scopeType"));
        assert!(json.contains("PARTNER"));
        assert!(json.contains("additionalClientIds"));
        assert!(json.contains("grantedClientIds"));
        assert!(json.contains("allowedRoleIds"));

        let deserialized: UpdateEmailDomainMappingCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.mapping_id, "edm-123");
        assert_eq!(deserialized.scope_type, Some("PARTNER".to_string()));
        assert_eq!(deserialized.sync_roles_from_idp, Some(true));
    }

    #[test]
    fn test_command_serialization_none_fields_skipped() {
        let cmd = UpdateEmailDomainMappingCommand {
            mapping_id: "edm-1".to_string(),
            scope_type: None,
            primary_client_id: None,
            sync_roles_from_idp: None,
            additional_client_ids: None,
            granted_client_ids: None,
            allowed_role_ids: None,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("mappingId"));
        assert!(!json.contains("scopeType"));
        assert!(!json.contains("primaryClientId"));
        assert!(!json.contains("syncRolesFromIdp"));
        assert!(!json.contains("additionalClientIds"));
        assert!(!json.contains("grantedClientIds"));
        assert!(!json.contains("allowedRoleIds"));
    }

    #[test]
    fn test_validate_empty_mapping_id() {
        let cmd = UpdateEmailDomainMappingCommand {
            mapping_id: "   ".to_string(),
            scope_type: None,
            primary_client_id: None,
            sync_roles_from_idp: None,
            additional_client_ids: None,
            granted_client_ids: None,
            allowed_role_ids: None,
        };
        assert!(cmd.mapping_id.trim().is_empty(), "Whitespace-only mapping_id should be treated as empty");
    }

    #[test]
    fn test_validate_valid_mapping_id() {
        let cmd = UpdateEmailDomainMappingCommand {
            mapping_id: "edm-123".to_string(),
            scope_type: None,
            primary_client_id: None,
            sync_roles_from_idp: None,
            additional_client_ids: None,
            granted_client_ids: None,
            allowed_role_ids: None,
        };
        assert!(!cmd.mapping_id.trim().is_empty());
    }
}
