//! Create Anchor Domain Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::AnchorDomainCreated;
use crate::auth::config_entity::AnchorDomain;
use crate::auth::config_repository::AnchorDomainRepository;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};

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
        if domain.is_empty() {
            return Err(UseCaseError::validation(
                "DOMAIN_REQUIRED",
                "Anchor domain is required",
            ));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &CreateAnchorDomainCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: CreateAnchorDomainCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<AnchorDomainCreated> {
        let domain = command.domain.trim().to_lowercase();

        // Business rule: domain must be unique
        let existing = match self.anchor_domain_repo.find_by_domain(&domain).await {
            Ok(found) => found,
            Err(e) => return UseCaseResult::failure(e.into()),
        };
        if existing.is_some() {
            return UseCaseResult::failure(UseCaseError::business_rule(
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
