//! Deactivate User Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::principal::repository::PrincipalRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::UserDeactivated;

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

    pub async fn execute(
        &self,
        command: DeactivateUserCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<UserDeactivated> {
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

        // Business rule: user must not already be deactivated
        if !principal.active {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "ALREADY_DEACTIVATED",
                "User is already deactivated",
            ));
        }

        // Deactivate the user
        principal.deactivate();

        // Create domain event
        let event = UserDeactivated::new(
            &ctx,
            &principal.id,
            command.reason.as_deref(),
        );

        // Atomic commit
        self.unit_of_work.commit(&principal, event, &command).await
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
