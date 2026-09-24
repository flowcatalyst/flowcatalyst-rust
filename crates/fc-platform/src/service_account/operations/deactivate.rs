//! Deactivate Service Account Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ServiceAccountDeactivated;
use crate::service_account::ServiceAccount;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::ServiceAccountRepository;

/// Command for deactivating a service account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeactivateServiceAccountCommand {
    /// Service account ID
    pub id: String,
}

/// Use case for deactivating a service account. Flips `active=false` on
/// the SA without touching its OAuth client — that's the caller's
/// responsibility (and the application-deactivate cascade does it
/// explicitly so the audit log records both).
pub struct DeactivateServiceAccountUseCase<U: UnitOfWork> {
    service_account_repo: Arc<ServiceAccountRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeactivateServiceAccountUseCase<U> {
    pub fn new(service_account_repo: Arc<ServiceAccountRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            service_account_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeactivateServiceAccountUseCase<U> {
    type Command = DeactivateServiceAccountCommand;
    type Event = ServiceAccountDeactivated;

    async fn validate(
        &self,
        _command: &DeactivateServiceAccountCommand,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &DeactivateServiceAccountCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: DeactivateServiceAccountCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ServiceAccountDeactivated> {
        let (sa, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit(&sa, &*self.service_account_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> DeactivateServiceAccountUseCase<U> {
    async fn prepare(
        &self,
        command: &DeactivateServiceAccountCommand,
        ctx: &ExecutionContext,
    ) -> Result<(ServiceAccount, ServiceAccountDeactivated), UseCaseError> {
        let mut sa = self
            .service_account_repo
            .find_by_id(&command.id)
            .await
            .or_not_found(
                "SERVICE_ACCOUNT_NOT_FOUND",
                format!("Service account with ID '{}' not found", command.id),
            )?;

        // Idempotent: already-inactive SA is a no-op success so the
        // cascade caller doesn't have to filter.
        if !sa.active {
            let event = ServiceAccountDeactivated::new(ctx, &sa.id, &sa.code);
            return Ok((sa, event));
        }

        sa.deactivate();

        let event = ServiceAccountDeactivated::new(ctx, &sa.id, &sa.code);
        Ok((sa, event))
    }
}
