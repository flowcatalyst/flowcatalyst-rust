//! Move an email-domain mapping to another identity provider (Go
//! `emaildomainmapping/operations/move_provider.go`). Moving to an INTERNAL
//! provider hands the domain's OIDC users back to it: provider INTERNAL,
//! external identity cleared, IdP-synced roles dropped, all in the same
//! transaction as the mapping. Moving to an OIDC provider resets nobody.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::email_domain_mapping::provider_move_repository::{ProviderMove, ProviderMoveRepository};
use crate::email_domain_mapping::repository::EmailDomainMappingRepository;
use crate::identity_provider::entity::IdentityProviderType;
use crate::identity_provider::repository::IdentityProviderRepository;
use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::PrincipalRepository;

/// Go `MappingProviderChanged`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailDomainMappingProviderChanged {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub mapping_id: String,
    pub email_domain: String,
    pub from_identity_provider_id: String,
    pub to_identity_provider_id: String,
    /// How many users went back to the internal provider (the response's
    /// `usersReset`; not part of the event data, as in Go).
    #[serde(skip)]
    pub users_reset: usize,
}

impl_domain_event!(EmailDomainMappingProviderChanged);

impl EmailDomainMappingProviderChanged {
    pub const EVENT_TYPE: &'static str = "platform:admin:email-domain-mapping:provider-changed";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveMappingToProviderCommand {
    pub mapping_id: String,
    pub identity_provider_id: String,
}

impl crate::usecase::AuditMasked for MoveMappingToProviderCommand {}

pub struct MoveMappingToProviderUseCase<U: UnitOfWork> {
    edm_repo: Arc<EmailDomainMappingRepository>,
    idp_repo: Arc<IdentityProviderRepository>,
    principal_repo: Arc<PrincipalRepository>,
    move_repo: Arc<ProviderMoveRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> MoveMappingToProviderUseCase<U> {
    pub fn new(
        edm_repo: Arc<EmailDomainMappingRepository>,
        idp_repo: Arc<IdentityProviderRepository>,
        principal_repo: Arc<PrincipalRepository>,
        move_repo: Arc<ProviderMoveRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            edm_repo,
            idp_repo,
            principal_repo,
            move_repo,
            unit_of_work,
        }
    }

    async fn prepare(
        &self,
        command: &MoveMappingToProviderCommand,
        ctx: &ExecutionContext,
    ) -> Result<(ProviderMove, EmailDomainMappingProviderChanged), UseCaseError> {
        let mapping = self
            .edm_repo
            .find_by_id(&command.mapping_id)
            .await
            .or_not_found(
                "EMAIL_DOMAIN_MAPPING_NOT_FOUND",
                format!("EmailDomainMapping not found: {}", command.mapping_id),
            )?;
        if mapping.identity_provider_id == command.identity_provider_id {
            return Err(UseCaseError::business_rule(
                "ALREADY_ON_PROVIDER",
                format!(
                    "Email domain '{}' is already mapped to that identity provider",
                    mapping.email_domain
                ),
            ));
        }
        let idp = self
            .idp_repo
            .find_by_id(&command.identity_provider_id)
            .await
            .or_not_found(
                "IDENTITY_PROVIDER_NOT_FOUND",
                format!(
                    "IdentityProvider not found: {}",
                    command.identity_provider_id
                ),
            )?;
        // Rust's rule (not Go's): a multi-tenant OIDC provider needs the
        // mapping pinned to a tenant.
        super::require_tenant_pin(idp.oidc_multi_tenant, &mapping)?;

        let reset_user_ids = if idp.r#type == IdentityProviderType::Internal {
            self.principal_repo
                .find_oidc_user_ids_by_email_domain(&mapping.email_domain)
                .await?
        } else {
            Vec::new()
        };
        let event = EmailDomainMappingProviderChanged {
            metadata: EventMetadata::from_ctx(
                ctx,
                EmailDomainMappingProviderChanged::EVENT_TYPE,
                "1.0",
                "platform:admin",
                format!("platform.emaildomainmapping.{}", mapping.id),
                format!("platform:emaildomainmapping:{}", mapping.id),
            ),
            mapping_id: mapping.id.clone(),
            email_domain: mapping.email_domain.clone(),
            from_identity_provider_id: mapping.identity_provider_id.clone(),
            to_identity_provider_id: command.identity_provider_id.clone(),
            users_reset: reset_user_ids.len(),
        };
        Ok((
            ProviderMove {
                mapping_id: mapping.id,
                identity_provider_id: command.identity_provider_id.clone(),
                reset_user_ids,
            },
            event,
        ))
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for MoveMappingToProviderUseCase<U> {
    type Command = MoveMappingToProviderCommand;
    type Event = EmailDomainMappingProviderChanged;

    async fn validate(&self, c: &MoveMappingToProviderCommand) -> Result<(), UseCaseError> {
        if c.mapping_id.trim().is_empty() {
            return Err(UseCaseError::validation("ID_REQUIRED", "id is required"));
        }
        if c.identity_provider_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "IDP_REQUIRED",
                "identityProviderId is required",
            ));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _c: &MoveMappingToProviderCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: MoveMappingToProviderCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<EmailDomainMappingProviderChanged> {
        let (mv, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };
        self.unit_of_work
            .commit(&mv, &*self.move_repo, event, &command)
            .await
    }
}
