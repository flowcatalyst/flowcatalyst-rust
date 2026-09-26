//! Delete Service Account Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ServiceAccountDeleted;
use crate::usecase::{Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError};
use crate::ServiceAccountRepository;

/// Command for deleting a service account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteServiceAccountCommand {
    /// Service account ID
    pub id: String,
}

impl crate::usecase::AuditMasked for DeleteServiceAccountCommand {}

/// Use case for deleting a service account.
pub struct DeleteServiceAccountUseCase<U: UnitOfWork> {
    service_account_repo: Arc<ServiceAccountRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteServiceAccountUseCase<U> {
    pub fn new(service_account_repo: Arc<ServiceAccountRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            service_account_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteServiceAccountUseCase<U> {
    type Command = DeleteServiceAccountCommand;
    type Event = ServiceAccountDeleted;

    async fn validate(&self, _command: &DeleteServiceAccountCommand) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &DeleteServiceAccountCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: DeleteServiceAccountCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<ServiceAccountDeleted>, UseCaseError> {
        // Find the service account
        let service_account = self
            .service_account_repo
            .find_by_id(&command.id)
            .await
            .or_not_found(
                "SERVICE_ACCOUNT_NOT_FOUND",
                format!("Service account with ID '{}' not found", command.id),
            )?;

        // Create domain event
        let event = ServiceAccountDeleted::new(&ctx, &service_account.id, &service_account.code);

        // Atomic commit with delete
        self.unit_of_work
            .commit_delete(
                &service_account,
                &*self.service_account_repo,
                event,
                &command,
            )
            .await
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
