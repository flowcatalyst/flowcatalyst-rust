//! Deactivate User Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::UserDeactivated;
use crate::principal::entity::Principal;
use crate::principal::repository::PrincipalRepository;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

/// Command for deactivating a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeactivateUserCommand {
    /// Principal ID to deactivate
    pub principal_id: String,

    /// Reason for deactivation (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Use case for deactivating an active user.
pub struct DeactivateUserUseCase<U: UnitOfWork> {
    principal_repo: Arc<PrincipalRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeactivateUserUseCase<U> {
    pub fn new(principal_repo: Arc<PrincipalRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            principal_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeactivateUserUseCase<U> {
    type Command = DeactivateUserCommand;
    type Event = UserDeactivated;

    async fn validate(&self, command: &DeactivateUserCommand) -> Result<(), UseCaseError> {
        if command.principal_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "PRINCIPAL_ID_REQUIRED",
                "Principal ID is required",
            ));
        }

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &DeactivateUserCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: DeactivateUserCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<UserDeactivated> {
        let (principal, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        // Atomic commit
        self.unit_of_work
            .commit(&principal, &*self.principal_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> DeactivateUserUseCase<U> {
    async fn prepare(
        &self,
        command: &DeactivateUserCommand,
        ctx: &ExecutionContext,
    ) -> Result<(Principal, UserDeactivated), UseCaseError> {
        // Fetch existing principal
        let mut principal = self
            .principal_repo
            .find_by_id(&command.principal_id)
            .await
            .or_not_found(
                "USER_NOT_FOUND",
                format!("User with ID '{}' not found", command.principal_id),
            )?;

        // Business rule: user must not already be deactivated
        if !principal.active {
            return Err(UseCaseError::business_rule(
                "ALREADY_DEACTIVATED",
                "User is already deactivated",
            ));
        }

        // Deactivate the user
        principal.deactivate();

        // Create domain event
        let event = UserDeactivated::new(ctx, &principal.id, command.reason.as_deref());
        Ok((principal, event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = DeactivateUserCommand {
            principal_id: "user-123".to_string(),
            reason: Some("Policy violation".to_string()),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("principalId"));
        assert!(json.contains("Policy violation"));
    }
}
