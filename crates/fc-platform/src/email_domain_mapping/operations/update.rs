//! Update Email Domain Mapping Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::EmailDomainMappingRepository;
use crate::email_domain_mapping::entity::ScopeType;
use crate::usecase::{ExecutionContext, UseCaseError, UseCaseResult};
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

pub struct UpdateEmailDomainMappingUseCase {
    edm_repo: Arc<EmailDomainMappingRepository>,
}

impl UpdateEmailDomainMappingUseCase {
    pub fn new(edm_repo: Arc<EmailDomainMappingRepository>) -> Self {
        Self { edm_repo }
    }

    pub async fn execute(
        &self,
        command: UpdateEmailDomainMappingCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<EmailDomainMappingUpdated> {
        if command.mapping_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "MAPPING_ID_REQUIRED", "Mapping ID is required",
            ));
        }

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

        UseCaseResult::success(event)
    }
}
