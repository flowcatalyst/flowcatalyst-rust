//! Deactivate Service Account Use Case

use async_trait::async_trait;
use fc_platform_core::shared::id::PrincipalId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ServiceAccountDeactivated;
use crate::service_account::repository::ServiceAccountRepository;
use crate::service_account::ServiceAccount;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for deactivating a service account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeactivateServiceAccountCommand {
    /// Service account ID
    pub id: PrincipalId,
}

impl AuditMasked for DeactivateServiceAccountCommand {}

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

    /// Service accounts are written by anchors only: an account's tier follows
    /// its client links, so a non-anchor could otherwise mint an ANCHOR-tier
    /// account (the `can_*_service_accounts` rules).
    /// The handler's gate checks this, with the permission, before the body
    /// is read; here it holds for every caller (fc-web, orchestrations).
    async fn authorize(
        &self,
        _command: &DeactivateServiceAccountCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(checks::require_anchor_scope(ctx.caller())?)
    }

    async fn execute(
        &self,
        command: DeactivateServiceAccountCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<ServiceAccountDeactivated>, UseCaseError> {
        let (sa, event) = self.prepare(&command, &ctx).await?;

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
            let event = ServiceAccountDeactivated::new(ctx, &sa);
            return Ok((sa, event));
        }

        sa.deactivate();

        let event = ServiceAccountDeactivated::new(ctx, &sa);
        Ok((sa, event))
    }
}
