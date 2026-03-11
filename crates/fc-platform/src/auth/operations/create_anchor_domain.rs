//! Create Anchor Domain Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::auth::config_entity::AnchorDomain;
use crate::auth::config_repository::AnchorDomainRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::AnchorDomainCreated;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAnchorDomainCommand {
    pub domain: String,
}

pub struct CreateAnchorDomainUseCase<U: UnitOfWork> {
    anchor_domain_repo: Arc<AnchorDomainRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateAnchorDomainUseCase<U> {
    pub fn new(anchor_domain_repo: Arc<AnchorDomainRepository>, unit_of_work: Arc<U>) -> Self {
        Self { anchor_domain_repo, unit_of_work }
    }

    pub async fn execute(
        &self,
        command: CreateAnchorDomainCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<AnchorDomainCreated> {
        let domain = command.domain.trim().to_lowercase();
        if domain.is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "DOMAIN_REQUIRED",
                "Anchor domain is required",
            ));
        }

        // Business rule: domain must be unique
        if let Ok(Some(_)) = self.anchor_domain_repo.find_by_domain(&domain).await {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "DOMAIN_EXISTS",
                format!("Anchor domain '{}' already exists", domain),
            ));
        }

        let anchor_domain = AnchorDomain::new(&domain);

        let event = AnchorDomainCreated::new(&ctx, &anchor_domain.id, &anchor_domain.domain);

        self.unit_of_work.commit(&anchor_domain, event, &command).await
    }
}
