//! Update Identity Provider Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::IdentityProviderUpdated;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::EmailDomainMappingRepository;
use crate::IdentityProviderRepository;

/// Command for updating an existing identity provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateIdentityProviderCommand {
    pub idp_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_issuer_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_client_secret_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_multi_tenant: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_issuer_pattern: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_email_domains: Option<Vec<String>>,
}

impl crate::usecase::AuditMasked for UpdateIdentityProviderCommand {}

/// Use case for updating an existing identity provider.
pub struct UpdateIdentityProviderUseCase<U: UnitOfWork> {
    idp_repo: Arc<IdentityProviderRepository>,
    edm_repo: Arc<EmailDomainMappingRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateIdentityProviderUseCase<U> {
    pub fn new(
        idp_repo: Arc<IdentityProviderRepository>,
        edm_repo: Arc<EmailDomainMappingRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            idp_repo,
            edm_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for UpdateIdentityProviderUseCase<U> {
    type Command = UpdateIdentityProviderCommand;
    type Event = IdentityProviderUpdated;

    async fn validate(&self, command: &UpdateIdentityProviderCommand) -> Result<(), UseCaseError> {
        super::require_sealed_secret(command.oidc_client_secret_ref.as_deref())
    }

    async fn authorize(
        &self,
        _command: &UpdateIdentityProviderCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: UpdateIdentityProviderCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<IdentityProviderUpdated> {
        let event = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work.emit_event(event, &command).await
    }
}

impl<U: UnitOfWork> UpdateIdentityProviderUseCase<U> {
    async fn prepare(
        &self,
        command: &UpdateIdentityProviderCommand,
        ctx: &ExecutionContext,
    ) -> Result<IdentityProviderUpdated, UseCaseError> {
        // Fetch existing identity provider
        let mut idp = self
            .idp_repo
            .find_by_id(&command.idp_id)
            .await
            .or_not_found(
                "NOT_FOUND",
                format!("Identity provider with ID '{}' not found", command.idp_id),
            )?;

        // Selectively update fields that are Some
        if let Some(ref name) = command.name {
            let name = name.trim();
            if name != idp.name {
                idp.name = name.to_string();
            }
        }

        if let Some(ref issuer_url) = command.oidc_issuer_url {
            idp.oidc_issuer_url = Some(issuer_url.clone());
        }

        if let Some(ref client_id) = command.oidc_client_id {
            idp.oidc_client_id = Some(client_id.clone());
        }

        if let Some(ref secret_ref) = command.oidc_client_secret_ref {
            idp.oidc_client_secret_ref = Some(secret_ref.clone());
        }

        if let Some(mt) = command.oidc_multi_tenant {
            idp.oidc_multi_tenant = mt;
        }
        if let Some(ref pattern) = command.oidc_issuer_pattern {
            idp.oidc_issuer_pattern = Some(pattern.clone());
        }
        if let Some(ref domains) = command.allowed_email_domains {
            idp.allowed_email_domains = domains.clone();
        }

        idp.updated_at = chrono::Utc::now();

        // A provider that is (or becomes) multi-tenant must not route any
        // domain whose mapping pins no tenant (owner ruling 2026-09-25,
        // item 3).
        if idp.oidc_multi_tenant {
            let unpinned = self
                .edm_repo
                .find_unpinned_domains_for_identity_provider(&idp.id)
                .await?;
            if !unpinned.is_empty() {
                return Err(UseCaseError::validation(
                    "TENANT_PIN_REQUIRED",
                    format!(
                        "A multi-tenant identity provider needs every mapped domain to pin its tenant; set requiredOidcTenantId on: {}",
                        unpinned.join(", ")
                    ),
                ));
            }
        }

        // Create domain event
        let event = IdentityProviderUpdated::new(ctx, &idp.id, &idp.code);

        // Update via repo
        if let Err(e) = self.idp_repo.update(&idp).await {
            return Err(UseCaseError::commit(format!(
                "Failed to update identity provider: {}",
                e
            )));
        }
        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = UpdateIdentityProviderCommand {
            idp_id: "idp-123".to_string(),
            name: Some("Updated Name".to_string()),
            oidc_issuer_url: None,
            oidc_client_id: None,
            oidc_client_secret_ref: None,
            oidc_multi_tenant: None,
            oidc_issuer_pattern: None,
            allowed_email_domains: None,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("idpId"));
        assert!(json.contains("Updated Name"));
    }
}
