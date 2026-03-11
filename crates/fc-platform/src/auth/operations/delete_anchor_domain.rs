//! Delete Anchor Domain Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::auth::config_repository::AnchorDomainRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::AnchorDomainDeleted;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAnchorDomainCommand {
    pub anchor_domain_id: String,
}

pub struct DeleteAnchorDomainUseCase<U: UnitOfWork> {
    anchor_domain_repo: Arc<AnchorDomainRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteAnchorDomainUseCase<U> {
    pub fn new(anchor_domain_repo: Arc<AnchorDomainRepository>, unit_of_work: Arc<U>) -> Self {
        Self { anchor_domain_repo, unit_of_work }
    }

    pub async fn execute(
        &self,
        command: DeleteAnchorDomainCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<AnchorDomainDeleted> {
        if command.anchor_domain_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "ID_REQUIRED",
                "Anchor domain ID is required",
            ));
        }

        let anchor_domain = match self.anchor_domain_repo.find_by_id(&command.anchor_domain_id).await {
            Ok(Some(ad)) => ad,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "ANCHOR_DOMAIN_NOT_FOUND",
                    format!("Anchor domain '{}' not found", command.anchor_domain_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch anchor domain: {}", e
                )));
            }
        };

        let event = AnchorDomainDeleted::new(&ctx, &anchor_domain.id, &anchor_domain.domain);

        self.unit_of_work.commit_delete(&anchor_domain, event, &command).await
    }
}
