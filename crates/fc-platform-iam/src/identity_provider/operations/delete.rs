//! Delete Identity Provider Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::IdentityProviderDeleted;
use crate::{
    email_domain_mapping::repository::EmailDomainMappingRepository,
    identity_provider::repository::IdentityProviderRepository,
};
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for deleting an identity provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteIdentityProviderCommand {
    pub idp_id: String,
}

impl fc_platform_core::usecase::AuditMasked for DeleteIdentityProviderCommand {}

/// Use case for deleting an identity provider.
pub struct DeleteIdentityProviderUseCase<U: UnitOfWork> {
    idp_repo: Arc<IdentityProviderRepository>,
    edm_repo: Arc<EmailDomainMappingRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteIdentityProviderUseCase<U> {
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
impl<U: UnitOfWork> UseCase for DeleteIdentityProviderUseCase<U> {
    type Command = DeleteIdentityProviderCommand;
    type Event = IdentityProviderDeleted;

    async fn validate(&self, command: &DeleteIdentityProviderCommand) -> Result<(), UseCaseError> {
        if command.idp_id.trim().is_empty() {
            return Err(UseCaseError::validation("ID_REQUIRED", "id is required"));
        }
        Ok(())
    }

    /// Identity providers are platform-owner data, written by anchors only
    /// (Go's `Can*IdentityProviders` are `anchorWith`).
    /// The handler's gate checks this, with the permission, before the body
    /// is read; here it holds for every caller (fc-web, orchestrations).
    async fn authorize(
        &self,
        _command: &DeleteIdentityProviderCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(
            fc_platform_core::shared::authorization_service::checks::require_anchor_scope(
                ctx.caller(),
            )?,
        )
    }

    async fn execute(
        &self,
        command: DeleteIdentityProviderCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<IdentityProviderDeleted>, UseCaseError> {
        // Fetch existing identity provider
        let idp = self
            .idp_repo
            .find_by_id(&command.idp_id)
            .await
            .or_not_found(
                "NOT_FOUND",
                format!("Identity provider with ID '{}' not found", command.idp_id),
            )?;

        // Go DeleteIdentityProvider: the seeded internal provider is the
        // fallback for released domains and cannot go; a provider still
        // routing domains cannot either (its users would silently fall to
        // the password prompt). Move or delete the mappings first.
        if idp.code == super::domains::INTERNAL_IDP_CODE {
            return Err(UseCaseError::business_rule(
                "INTERNAL_IDP_PROTECTED",
                "The internal identity provider cannot be deleted",
            ));
        }
        let mapped = self.edm_repo.find_by_identity_provider(&idp.id).await?;
        if !mapped.is_empty() {
            let domains: Vec<&str> = mapped.iter().map(|m| m.email_domain.as_str()).collect();
            return Err(UseCaseError::business_rule(
                "DOMAINS_STILL_MAPPED",
                format!(
                    "Identity provider still routes email domains ({}); move or delete those mappings first",
                    domains.join(", ")
                ),
            ));
        }

        let event = IdentityProviderDeleted::new(&ctx, &idp.id, &idp.code);

        self.unit_of_work
            .commit_delete(&idp, &*self.idp_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = DeleteIdentityProviderCommand {
            idp_id: "idp-123".to_string(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("idpId"));
    }
}
