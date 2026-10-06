//! Activate User Use Case

use async_trait::async_trait;
use fc_platform_core::shared::id::PrincipalId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::UserActivated;
use crate::principal::repository::PrincipalRepository;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for activating a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivateUserCommand {
    /// Principal ID to activate
    pub principal_id: PrincipalId,
}

impl AuditMasked for ActivateUserCommand {}

/// Use case for activating a deactivated user.
pub struct ActivateUserUseCase<U: UnitOfWork> {
    principal_repo: Arc<PrincipalRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> ActivateUserUseCase<U> {
    pub fn new(principal_repo: Arc<PrincipalRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            principal_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for ActivateUserUseCase<U> {
    type Command = ActivateUserCommand;
    type Event = UserActivated;

    async fn validate(&self, command: &ActivateUserCommand) -> Result<(), UseCaseError> {
        if command.principal_id.as_str().trim().is_empty() {
            return Err(UseCaseError::validation(
                "PRINCIPAL_ID_REQUIRED",
                "Principal ID is required",
            ));
        }

        Ok(())
    }

    /// The target must be a user the caller administers (Go
    /// `requireUserResourceAccess`, post-load): a client administrator manages
    /// only CLIENT-tier users (403) of a client it reaches (else
    /// `Principal_NOT_FOUND`, as a missing id). The coarse `can_write_principals` gate stays in
    /// the handler, before anything is loaded.
    async fn authorize(
        &self,
        command: &ActivateUserCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        super::access::load_administered_user(
            &self.principal_repo,
            ctx.caller(),
            &command.principal_id,
            "Principal",
        )
        .await?;
        Ok(())
    }

    async fn execute(
        &self,
        command: ActivateUserCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<UserActivated>, UseCaseError> {
        // Fetch existing principal
        let mut principal = self
            .principal_repo
            .find_by_id(&command.principal_id)
            .await
            .or_not_found(
                "PRINCIPAL_NOT_FOUND",
                format!("User with ID '{}' not found", command.principal_id),
            )?;

        // Business rule: user must not already be active
        if principal.active {
            return Err(UseCaseError::business_rule(
                "ALREADY_ACTIVE",
                "User is already active",
            ));
        }

        // Activate the user
        principal.activate();

        // Create domain event
        let event = UserActivated::new(&ctx, &principal.id);

        // Atomic commit
        self.unit_of_work
            .commit(&principal, &*self.principal_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = ActivateUserCommand {
            principal_id: PrincipalId::from_wire("user-123"),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("principalId"));
    }
}
