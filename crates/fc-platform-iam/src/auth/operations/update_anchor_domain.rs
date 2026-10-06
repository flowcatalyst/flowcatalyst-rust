//! Update Anchor Domain Use Case

use async_trait::async_trait;
use fc_platform_core::shared::id::AnchorDomainId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::AnchorDomainUpdated;
use crate::auth::config_repository::AnchorDomainRepository;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAnchorDomainCommand {
    pub anchor_domain_id: AnchorDomainId,
    pub domain: String,
}

impl AuditMasked for UpdateAnchorDomainCommand {}

pub struct UpdateAnchorDomainUseCase<U: UnitOfWork> {
    anchor_domain_repo: Arc<AnchorDomainRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateAnchorDomainUseCase<U> {
    pub fn new(anchor_domain_repo: Arc<AnchorDomainRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            anchor_domain_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for UpdateAnchorDomainUseCase<U> {
    type Command = UpdateAnchorDomainCommand;
    type Event = AnchorDomainUpdated;

    async fn validate(&self, command: &UpdateAnchorDomainCommand) -> Result<(), UseCaseError> {
        if command.anchor_domain_id.as_str().trim().is_empty() {
            return Err(UseCaseError::validation(
                "ID_REQUIRED",
                "Anchor domain ID is required",
            ));
        }
        // Go UpdateAnchorDomain: blank or malformed is one INVALID_DOMAIN.
        let domain = command.domain.trim().to_lowercase();
        if !super::create_anchor_domain::is_dns_name(&domain) {
            return Err(UseCaseError::validation(
                "INVALID_DOMAIN",
                "domain must be a valid DNS name",
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
        _command: &UpdateAnchorDomainCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(checks::require_anchor_scope(ctx.caller())?)
    }

    async fn execute(
        &self,
        command: UpdateAnchorDomainCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<AnchorDomainUpdated>, UseCaseError> {
        let mut anchor_domain = self
            .anchor_domain_repo
            .find_by_id(&command.anchor_domain_id)
            .await
            .or_not_found(
                "ANCHOR_DOMAIN_NOT_FOUND",
                format!("Anchor domain '{}' not found", command.anchor_domain_id),
            )?;

        let new_domain = command.domain.trim().to_lowercase();

        // Business rule: new domain must be unique (unless it's the same row).
        if new_domain != anchor_domain.domain {
            if let Some(other) = self.anchor_domain_repo.find_by_domain(&new_domain).await? {
                if other.id != anchor_domain.id {
                    return Err(UseCaseError::business_rule(
                        "DOMAIN_EXISTS",
                        format!("Anchor domain '{}' already exists", new_domain),
                    ));
                }
            }
        }

        anchor_domain.domain = new_domain.clone();
        anchor_domain.updated_at = chrono::Utc::now();

        let event = AnchorDomainUpdated::new(&ctx, &anchor_domain.id, &new_domain);

        self.unit_of_work
            .commit(&anchor_domain, &*self.anchor_domain_repo, event, &command)
            .await
    }
}
