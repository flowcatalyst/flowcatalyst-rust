//! Update Anchor Domain Use Case

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::auth::config_repository::AnchorDomainRepository;
use crate::usecase::{
    ExecutionContext, UseCase, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::AnchorDomainUpdated;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAnchorDomainCommand {
    pub anchor_domain_id: String,
    pub domain: String,
}

pub struct UpdateAnchorDomainUseCase<U: UnitOfWork> {
    anchor_domain_repo: Arc<AnchorDomainRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateAnchorDomainUseCase<U> {
    pub fn new(anchor_domain_repo: Arc<AnchorDomainRepository>, unit_of_work: Arc<U>) -> Self {
        Self { anchor_domain_repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for UpdateAnchorDomainUseCase<U> {
    type Command = UpdateAnchorDomainCommand;
    type Event = AnchorDomainUpdated;

    async fn validate(&self, command: &UpdateAnchorDomainCommand) -> Result<(), UseCaseError> {
        if command.anchor_domain_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "ID_REQUIRED", "Anchor domain ID is required",
            ));
        }
        if command.domain.trim().is_empty() {
            return Err(UseCaseError::validation(
                "DOMAIN_REQUIRED", "Domain is required",
            ));
        }
        Ok(())
    }

    async fn authorize(&self, _command: &UpdateAnchorDomainCommand, _ctx: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: UpdateAnchorDomainCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<AnchorDomainUpdated> {
        let mut anchor_domain = match self.anchor_domain_repo.find_by_id(&command.anchor_domain_id).await {
            Ok(Some(ad)) => ad,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "ANCHOR_DOMAIN_NOT_FOUND",
                    format!("Anchor domain '{}' not found", command.anchor_domain_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch anchor domain: {}", e,
                )));
            }
        };

        let new_domain = command.domain.trim().to_lowercase();

        // Business rule: new domain must be unique (unless it's the same row).
        if new_domain != anchor_domain.domain {
            if let Ok(Some(other)) = self.anchor_domain_repo.find_by_domain(&new_domain).await {
                if other.id != anchor_domain.id {
                    return UseCaseResult::failure(UseCaseError::business_rule(
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
