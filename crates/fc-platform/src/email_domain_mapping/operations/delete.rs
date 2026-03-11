//! Delete Email Domain Mapping Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::EmailDomainMappingRepository;
use crate::usecase::{ExecutionContext, UseCaseError, UseCaseResult};
use super::events::EmailDomainMappingDeleted;

/// Command for deleting an email domain mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteEmailDomainMappingCommand {
    pub mapping_id: String,
}

pub struct DeleteEmailDomainMappingUseCase {
    edm_repo: Arc<EmailDomainMappingRepository>,
}

impl DeleteEmailDomainMappingUseCase {
    pub fn new(edm_repo: Arc<EmailDomainMappingRepository>) -> Self {
        Self { edm_repo }
    }

    pub async fn execute(
        &self,
        command: DeleteEmailDomainMappingCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<EmailDomainMappingDeleted> {
        if command.mapping_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "MAPPING_ID_REQUIRED", "Mapping ID is required",
            ));
        }

        let mapping = match self.edm_repo.find_by_id(&command.mapping_id).await {
            Ok(Some(m)) => m,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "NOT_FOUND",
                    format!("Email domain mapping with ID '{}' not found", command.mapping_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch email domain mapping: {}", e
                )));
            }
        };

        if let Err(e) = self.edm_repo.delete(&mapping.id).await {
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to delete email domain mapping: {}", e
            )));
        }

        let event = EmailDomainMappingDeleted::new(
            &ctx,
            &mapping.id,
            &mapping.email_domain,
        );

        UseCaseResult::success(event)
    }
}
