//! Activate User Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::principal::repository::PrincipalRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::UserActivated;

/// Command for activating a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivateUserCommand {
    /// Principal ID to activate
    pub principal_id: String,
}

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

    pub async fn execute(
        &self,
        command: ActivateUserCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<UserActivated> {
        // Validation: principal_id is required
        if command.principal_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "PRINCIPAL_ID_REQUIRED",
                "Principal ID is required",
            ));
        }

        // Fetch existing principal
        let mut principal = match self.principal_repo.find_by_id(&command.principal_id).await {
            Ok(Some(p)) => p,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "USER_NOT_FOUND",
                    format!("User with ID '{}' not found", command.principal_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch user: {}",
                    e
                )));
            }
        };

        // Business rule: user must not already be active
        if principal.active {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "ALREADY_ACTIVE",
                "User is already active",
            ));
        }

        // Activate the user
        principal.activate();

        // Create domain event
        let event = UserActivated::new(&ctx, &principal.id);

        // Atomic commit
        self.unit_of_work.commit(&principal, event, &command).await
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
