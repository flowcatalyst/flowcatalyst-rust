//! Delete Service Account Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::ServiceAccountRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::ServiceAccountDeleted;

/// Command for deleting a service account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteServiceAccountCommand {
    /// Service account ID
    pub id: String,
}

/// Use case for deleting a service account.
pub struct DeleteServiceAccountUseCase<U: UnitOfWork> {
    service_account_repo: Arc<ServiceAccountRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteServiceAccountUseCase<U> {
    pub fn new(
        service_account_repo: Arc<ServiceAccountRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            service_account_repo,
            unit_of_work,
        }
    }

    pub async fn execute(
        &self,
        command: DeleteServiceAccountCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ServiceAccountDeleted> {
        // Find the service account
        let service_account = match self.service_account_repo.find_by_id(&command.id).await {
            Ok(Some(sa)) => sa,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "SERVICE_ACCOUNT_NOT_FOUND",
                    format!("Service account with ID '{}' not found", command.id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(
                    format!("Failed to find service account: {}", e),
                ));
            }
        };

        // Create domain event
        let event = ServiceAccountDeleted::new(
            &ctx,
            &service_account.id,
            &service_account.code,
        );

        // Atomic commit with delete
        self.unit_of_work.commit_delete(&service_account, event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = DeleteServiceAccountCommand {
            id: "sa-123".to_string(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("sa-123"));
    }
}
