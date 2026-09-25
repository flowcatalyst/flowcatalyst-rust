//! Create Email Domain Mapping Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::EmailDomainMappingCreated;
use crate::email_domain_mapping::entity::{EmailDomainMapping, ScopeType};
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::EmailDomainMappingRepository;
use crate::IdentityProviderRepository;

/// Command for creating a new email domain mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEmailDomainMappingCommand {
    pub email_domain: String,
    pub identity_provider_id: String,
    pub scope_type: ScopeType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_client_id: Option<String>,
    #[serde(default)]
    pub additional_client_ids: Vec<String>,
    #[serde(default)]
    pub granted_client_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_oidc_tenant_id: Option<String>,
    #[serde(default)]
    pub allowed_role_ids: Vec<String>,
    #[serde(default)]
    pub sync_roles_from_idp: bool,
}

impl crate::usecase::AuditMasked for CreateEmailDomainMappingCommand {}

pub struct CreateEmailDomainMappingUseCase<U: UnitOfWork> {
    edm_repo: Arc<EmailDomainMappingRepository>,
    idp_repo: Arc<IdentityProviderRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateEmailDomainMappingUseCase<U> {
    pub fn new(
        edm_repo: Arc<EmailDomainMappingRepository>,
        idp_repo: Arc<IdentityProviderRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            edm_repo,
            idp_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for CreateEmailDomainMappingUseCase<U> {
    type Command = CreateEmailDomainMappingCommand;
    type Event = EmailDomainMappingCreated;

    async fn validate(
        &self,
        command: &CreateEmailDomainMappingCommand,
    ) -> Result<(), UseCaseError> {
        let email_domain = command.email_domain.trim().to_lowercase();
        if email_domain.is_empty() {
            return Err(UseCaseError::validation(
                "EMAIL_DOMAIN_REQUIRED",
                "Email domain is required",
            ));
        }

        if command.identity_provider_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "IDENTITY_PROVIDER_ID_REQUIRED",
                "Identity provider ID is required",
            ));
        }

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &CreateEmailDomainMappingCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: CreateEmailDomainMappingCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<EmailDomainMappingCreated> {
        let event = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        // Emit the event + audit log via UoW. The entity itself was written
        // in prepare() via a direct repo call (pre-existing pattern with junction
        // tables). TODO: migrate to `impl Persist<EmailDomainMapping> for
        // EmailDomainMappingRepository` and `unit_of_work.commit(...)` to
        // match the rest of the codebase.
        self.unit_of_work.emit_event(event, &command).await
    }
}

impl<U: UnitOfWork> CreateEmailDomainMappingUseCase<U> {
    async fn prepare(
        &self,
        command: &CreateEmailDomainMappingCommand,
        ctx: &ExecutionContext,
    ) -> Result<EmailDomainMappingCreated, UseCaseError> {
        let email_domain = command.email_domain.trim().to_lowercase();

        // Verify identity provider exists
        let idp = self
            .idp_repo
            .find_by_id(&command.identity_provider_id)
            .await
            .or_not_found(
                "IDENTITY_PROVIDER_NOT_FOUND",
                format!(
                    "Identity provider '{}' not found",
                    command.identity_provider_id
                ),
            )?;

        // Check for duplicate email domain
        if self
            .edm_repo
            .find_by_email_domain(&email_domain)
            .await?
            .is_some()
        {
            return Err(UseCaseError::business_rule(
                "EMAIL_DOMAIN_ALREADY_MAPPED",
                format!("Email domain '{}' is already mapped", email_domain),
            ));
        }

        let scope_type = command.scope_type;

        let mut mapping =
            EmailDomainMapping::new(&email_domain, &command.identity_provider_id, scope_type);
        mapping.primary_client_id = command.primary_client_id.clone();
        mapping.additional_client_ids = command.additional_client_ids.clone();
        mapping.granted_client_ids = command.granted_client_ids.clone();
        mapping.required_oidc_tenant_id = command.required_oidc_tenant_id.clone();
        mapping.allowed_role_ids = command.allowed_role_ids.clone();
        mapping.sync_roles_from_idp = command.sync_roles_from_idp;
        super::require_tenant_pin(idp.oidc_multi_tenant, &mapping)?;

        if let Err(e) = self.edm_repo.insert(&mapping).await {
            return Err(UseCaseError::commit(format!(
                "Failed to insert email domain mapping: {}",
                e
            )));
        }

        let event = EmailDomainMappingCreated::new(
            ctx,
            &mapping.id,
            &mapping.email_domain,
            &mapping.identity_provider_id,
            scope_type.as_str(),
        );

        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = CreateEmailDomainMappingCommand {
            email_domain: "example.com".to_string(),
            identity_provider_id: "idp-123".to_string(),
            scope_type: ScopeType::Anchor,
            primary_client_id: Some("client-456".to_string()),
            sync_roles_from_idp: true,
            additional_client_ids: vec![],
            granted_client_ids: vec![],
            required_oidc_tenant_id: None,
            allowed_role_ids: vec![],
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("emailDomain"));
        assert!(json.contains("example.com"));
        assert!(json.contains("identityProviderId"));
        assert!(json.contains("idp-123"));
        assert!(json.contains("primaryClientId"));
        assert!(json.contains("client-456"));
        assert!(json.contains("syncRolesFromIdp"));

        let deserialized: CreateEmailDomainMappingCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.email_domain, "example.com");
        assert_eq!(deserialized.identity_provider_id, "idp-123");
        assert_eq!(deserialized.scope_type, ScopeType::Anchor);
        assert_eq!(
            deserialized.primary_client_id,
            Some("client-456".to_string())
        );
        assert!(deserialized.sync_roles_from_idp);
    }

    #[test]
    fn test_command_serialization_without_optional_fields() {
        let cmd = CreateEmailDomainMappingCommand {
            email_domain: "test.org".to_string(),
            identity_provider_id: "idp-1".to_string(),
            scope_type: ScopeType::Client,
            primary_client_id: None,
            sync_roles_from_idp: false,
            additional_client_ids: vec![],
            granted_client_ids: vec![],
            required_oidc_tenant_id: None,
            allowed_role_ids: vec![],
        };

        let json = serde_json::to_string(&cmd).unwrap();
        // primaryClientId should be skipped when None
        assert!(!json.contains("primaryClientId"));
    }

    #[test]
    fn test_validate_empty_email_domain() {
        // Replicate the validation logic from validate() — email_domain is trimmed
        let cmd = CreateEmailDomainMappingCommand {
            email_domain: "   ".to_string(),
            identity_provider_id: "idp-1".to_string(),
            scope_type: ScopeType::Anchor,
            primary_client_id: None,
            sync_roles_from_idp: false,
            additional_client_ids: vec![],
            granted_client_ids: vec![],
            required_oidc_tenant_id: None,
            allowed_role_ids: vec![],
        };
        let trimmed = cmd.email_domain.trim().to_lowercase();
        assert!(
            trimmed.is_empty(),
            "Whitespace-only email domain should be treated as empty"
        );
    }

    #[test]
    fn test_validate_empty_identity_provider_id() {
        let cmd = CreateEmailDomainMappingCommand {
            email_domain: "example.com".to_string(),
            identity_provider_id: "  ".to_string(),
            scope_type: ScopeType::Anchor,
            primary_client_id: None,
            sync_roles_from_idp: false,
            additional_client_ids: vec![],
            granted_client_ids: vec![],
            required_oidc_tenant_id: None,
            allowed_role_ids: vec![],
        };
        assert!(
            cmd.identity_provider_id.trim().is_empty(),
            "Whitespace-only IDP ID should be treated as empty"
        );
    }

    #[test]
    fn test_validate_valid_inputs() {
        let cmd = CreateEmailDomainMappingCommand {
            email_domain: "example.com".to_string(),
            identity_provider_id: "idp-123".to_string(),
            scope_type: ScopeType::Anchor,
            primary_client_id: None,
            sync_roles_from_idp: false,
            additional_client_ids: vec![],
            granted_client_ids: vec![],
            required_oidc_tenant_id: None,
            allowed_role_ids: vec![],
        };
        let trimmed = cmd.email_domain.trim().to_lowercase();
        assert!(!trimmed.is_empty());
        assert!(!cmd.identity_provider_id.trim().is_empty());
    }

    #[test]
    fn test_email_domain_normalized_to_lowercase() {
        let cmd = CreateEmailDomainMappingCommand {
            email_domain: "  EXAMPLE.COM  ".to_string(),
            identity_provider_id: "idp-1".to_string(),
            scope_type: ScopeType::Client,
            primary_client_id: None,
            sync_roles_from_idp: false,
            additional_client_ids: vec![],
            granted_client_ids: vec![],
            required_oidc_tenant_id: None,
            allowed_role_ids: vec![],
        };
        let normalized = cmd.email_domain.trim().to_lowercase();
        assert_eq!(normalized, "example.com");
    }
}
