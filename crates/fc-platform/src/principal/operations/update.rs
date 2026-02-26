//! Update User Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::principal::repository::PrincipalRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::UserUpdated;

/// Command for updating an existing user.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserCommand {
    /// Principal ID to update
    pub principal_id: String,

    /// New display name (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Use case for updating an existing user.
pub struct UpdateUserUseCase<U: UnitOfWork> {
    principal_repo: Arc<PrincipalRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateUserUseCase<U> {
    pub fn new(principal_repo: Arc<PrincipalRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            principal_repo,
            unit_of_work,
        }
    }

    pub async fn execute(
        &self,
        command: UpdateUserCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<UserUpdated> {
        // Validation: principal_id is required
        if command.principal_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "PRINCIPAL_ID_REQUIRED",
                "Principal ID is required",
            ));
        }

        // Validation: at least one field to update
        if command.name.is_none() {
            return UseCaseResult::failure(UseCaseError::validation(
                "NO_UPDATES",
                "At least one field must be provided for update",
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

        // Business rule: can only update USER type principals
        if !principal.is_user() {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "NOT_A_USER",
                "Can only update user-type principals",
            ));
        }

        // Apply updates
        let mut updated_name: Option<&str> = None;

        if let Some(ref name) = command.name {
            let name = name.trim();
            if name != principal.name {
                principal.name = name.to_string();
                updated_name = Some(name);
            }
        }

        // Check if anything actually changed
        if updated_name.is_none() {
            return UseCaseResult::failure(UseCaseError::validation(
                "NO_CHANGES",
                "No changes detected",
            ));
        }

        principal.updated_at = chrono::Utc::now();

        // Create domain event
        let event = UserUpdated::new(&ctx, &principal.id, updated_name, None);

        // Atomic commit
        self.unit_of_work.commit(&principal, event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = UpdateUserCommand {
            principal_id: "user-123".to_string(),
            name: Some("New Name".to_string()),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("principalId"));
        assert!(json.contains("New Name"));
    }
}
