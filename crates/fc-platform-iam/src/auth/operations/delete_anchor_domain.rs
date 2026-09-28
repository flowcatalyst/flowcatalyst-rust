//! Delete Anchor Domain Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::AnchorDomainDeleted;
use crate::auth::config_repository::AnchorDomainRepository;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAnchorDomainCommand {
    pub anchor_domain_id: String,
}

impl fc_platform_core::usecase::AuditMasked for DeleteAnchorDomainCommand {}

pub struct DeleteAnchorDomainUseCase<U: UnitOfWork> {
    anchor_domain_repo: Arc<AnchorDomainRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteAnchorDomainUseCase<U> {
    pub fn new(anchor_domain_repo: Arc<AnchorDomainRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            anchor_domain_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteAnchorDomainUseCase<U> {
    type Command = DeleteAnchorDomainCommand;
    type Event = AnchorDomainDeleted;

    async fn validate(&self, command: &DeleteAnchorDomainCommand) -> Result<(), UseCaseError> {
        if command.anchor_domain_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "ID_REQUIRED",
                "Anchor domain ID is required",
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
        _command: &DeleteAnchorDomainCommand,
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
        command: DeleteAnchorDomainCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<AnchorDomainDeleted>, UseCaseError> {
        let anchor_domain = self
            .anchor_domain_repo
            .find_by_id(&command.anchor_domain_id)
            .await
            .or_not_found(
                "ANCHOR_DOMAIN_NOT_FOUND",
                format!("Anchor domain '{}' not found", command.anchor_domain_id),
            )?;

        let event = AnchorDomainDeleted::new(&ctx, &anchor_domain.id, &anchor_domain.domain);

        self.unit_of_work
            .commit_delete(&anchor_domain, &*self.anchor_domain_repo, event, &command)
            .await
    }
}
