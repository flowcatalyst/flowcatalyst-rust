//! Create Email Domain Mapping Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::EmailDomainMappingRepository;
use crate::IdentityProviderRepository;
use crate::email_domain_mapping::entity::{EmailDomainMapping, ScopeType};
use crate::usecase::{ExecutionContext, UseCaseError, UseCaseResult};
use super::events::EmailDomainMappingCreated;

/// Command for creating a new email domain mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEmailDomainMappingCommand {
    pub email_domain: String,
    pub identity_provider_id: String,
    pub scope_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_client_id: Option<String>,
    #[serde(default)]
    pub sync_roles_from_idp: bool,
}

pub struct CreateEmailDomainMappingUseCase {
    edm_repo: Arc<EmailDomainMappingRepository>,
    idp_repo: Arc<IdentityProviderRepository>,
}

impl CreateEmailDomainMappingUseCase {
    pub fn new(
        edm_repo: Arc<EmailDomainMappingRepository>,
        idp_repo: Arc<IdentityProviderRepository>,
    ) -> Self {
        Self { edm_repo, idp_repo }
    }

    pub async fn execute(
        &self,
        command: CreateEmailDomainMappingCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<EmailDomainMappingCreated> {
        let email_domain = command.email_domain.trim().to_lowercase();
        if email_domain.is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "EMAIL_DOMAIN_REQUIRED", "Email domain is required",
            ));
        }

        if command.identity_provider_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "IDENTITY_PROVIDER_ID_REQUIRED", "Identity provider ID is required",
            ));
        }

        // Verify identity provider exists
        match self.idp_repo.find_by_id(&command.identity_provider_id).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "IDENTITY_PROVIDER_NOT_FOUND",
                    format!("Identity provider '{}' not found", command.identity_provider_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to validate identity provider: {}", e
                )));
            }
        }

        // Check for duplicate email domain
        match self.edm_repo.find_by_email_domain(&email_domain).await {
            Ok(Some(_)) => {
                return UseCaseResult::failure(UseCaseError::business_rule(
                    "EMAIL_DOMAIN_ALREADY_MAPPED",
                    format!("Email domain '{}' is already mapped", email_domain),
                ));
            }
            Ok(None) => {}
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to check email domain: {}", e
                )));
            }
        }

        // Parse scope type
        let scope_type = match command.scope_type.to_uppercase().as_str() {
            "ANCHOR" => ScopeType::Anchor,
            "PARTNER" => ScopeType::Partner,
            _ => ScopeType::Client,
        };

        let mut mapping = EmailDomainMapping::new(&email_domain, &command.identity_provider_id, scope_type);
        mapping.primary_client_id = command.primary_client_id.clone();
        mapping.sync_roles_from_idp = command.sync_roles_from_idp;

        if let Err(e) = self.edm_repo.insert(&mapping).await {
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to insert email domain mapping: {}", e
            )));
        }

        let event = EmailDomainMappingCreated::new(
            &ctx,
            &mapping.id,
            &mapping.email_domain,
            &mapping.identity_provider_id,
            scope_type.as_str(),
        );

        UseCaseResult::success(event)
    }
}
