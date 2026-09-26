//! Activate User Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::UserActivated;
use crate::principal::repository::PrincipalRepository;
use crate::usecase::{Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError};

/// Command for activating a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivateUserCommand {
    /// Principal ID to activate
    pub principal_id: String,
}

impl crate::usecase::AuditMasked for ActivateUserCommand {}

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
        _command: &ActivateUserCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
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
            principal_id: "user-123".to_string(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("principalId"));
    }
}
