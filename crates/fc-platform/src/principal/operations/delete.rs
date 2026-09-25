//! Delete User Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::UserDeleted;
use crate::principal::entity::Principal;
use crate::principal::repository::PrincipalRepository;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

/// Command for deleting a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteUserCommand {
    /// Principal ID to delete
    pub principal_id: String,
}

impl crate::usecase::AuditMasked for DeleteUserCommand {}

/// Use case for deleting a user (soft delete - deactivates permanently).
pub struct DeleteUserUseCase<U: UnitOfWork> {
    principal_repo: Arc<PrincipalRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteUserUseCase<U> {
    pub fn new(principal_repo: Arc<PrincipalRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            principal_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteUserUseCase<U> {
    type Command = DeleteUserCommand;
    type Event = UserDeleted;

    async fn validate(&self, command: &DeleteUserCommand) -> Result<(), UseCaseError> {
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
        _command: &DeleteUserCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: DeleteUserCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<UserDeleted> {
        let (principal, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        // Atomic commit with delete
        self.unit_of_work
            .commit_delete(&principal, &*self.principal_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> DeleteUserUseCase<U> {
    async fn prepare(
        &self,
        command: &DeleteUserCommand,
        ctx: &ExecutionContext,
    ) -> Result<(Principal, UserDeleted), UseCaseError> {
        // Business rule: cannot delete yourself
        if command.principal_id == ctx.principal_id {
            return Err(UseCaseError::business_rule(
                "CANNOT_DELETE_SELF",
                "Cannot delete your own account",
            ));
        }

        // Fetch existing principal
        let principal = self
            .principal_repo
            .find_by_id(&command.principal_id)
            .await
            .or_not_found(
                "PRINCIPAL_NOT_FOUND",
                format!("User with ID '{}' not found", command.principal_id),
            )?;

        // Create domain event
        let event = UserDeleted::new(ctx, &principal.id, principal.email().unwrap_or(""));
        Ok((principal, event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = DeleteUserCommand {
            principal_id: "user-123".to_string(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("principalId"));
    }
}
