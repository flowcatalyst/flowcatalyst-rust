//! Create Anchor Domain Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::AnchorDomainCreated;
use crate::auth::config_entity::AnchorDomain;
use crate::auth::config_repository::AnchorDomainRepository;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{Committed, ExecutionContext, UnitOfWork, UseCase, UseCaseError};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAnchorDomainCommand {
    pub domain: String,
}

impl AuditMasked for CreateAnchorDomainCommand {}

pub struct CreateAnchorDomainUseCase<U: UnitOfWork> {
    anchor_domain_repo: Arc<AnchorDomainRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateAnchorDomainUseCase<U> {
    pub fn new(anchor_domain_repo: Arc<AnchorDomainRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            anchor_domain_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for CreateAnchorDomainUseCase<U> {
    type Command = CreateAnchorDomainCommand;
    type Event = AnchorDomainCreated;

    async fn validate(&self, command: &CreateAnchorDomainCommand) -> Result<(), UseCaseError> {
        let domain = command.domain.trim().to_lowercase();
        // Go CreateAnchorDomain (auth/operations/anchor_domain.go): blank is
        // DOMAIN_REQUIRED; a name without a dot, or with a space, `/` or `@`,
        // is INVALID_DOMAIN.
        if domain.is_empty() {
            return Err(UseCaseError::validation(
                "DOMAIN_REQUIRED",
                "domain is required",
            ));
        }
        if !is_dns_name(&domain) {
            return Err(UseCaseError::validation(
                "INVALID_DOMAIN",
                "domain must be a valid DNS name (e.g. example.com)",
            ));
        }
        Ok(())
    }

    /// Anchor domains are platform-owner data, written by anchors only (Go's
    /// `Can*AnchorDomains` are `anchorWith`).
    /// The handler's gate checks this, with the permission, before the body
    /// is read; here it holds for every caller (fc-web, orchestrations).
    async fn authorize(
        &self,
        _command: &CreateAnchorDomainCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(checks::require_anchor_scope(ctx.caller())?)
    }

    async fn execute(
        &self,
        command: CreateAnchorDomainCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<AnchorDomainCreated>, UseCaseError> {
        let domain = command.domain.trim().to_lowercase();

        // Business rule: domain must be unique
        let existing = self.anchor_domain_repo.find_by_domain(&domain).await?;
        if existing.is_some() {
            return Err(UseCaseError::business_rule(
                "DOMAIN_EXISTS",
                format!("Anchor domain '{}' already exists", domain),
            ));
        }

        let anchor_domain = AnchorDomain::new(&domain);

        let event = AnchorDomainCreated::new(&ctx, &anchor_domain.id, &anchor_domain.domain);

        self.unit_of_work
            .commit(&anchor_domain, &*self.anchor_domain_repo, event, &command)
            .await
    }
}

/// Go's anchor-domain shape check: at least one dot, and no space, `/` or
/// `@` (auth/operations/anchor_domain.go).
pub(crate) fn is_dns_name(domain: &str) -> bool {
    domain.contains('.') && !domain.contains([' ', '/', '@'])
}
