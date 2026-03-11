//! Assign Application Access Use Case
//!
//! Sets which applications a user can access.
//! Computes delta (added/removed) and persists via UnitOfWork.

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::PrincipalRepository;
use crate::ApplicationRepository;
use crate::principal::entity::PrincipalType;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::ApplicationAccessAssigned;

/// Command for assigning application access to a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignApplicationAccessCommand {
    pub user_id: String,
    pub application_ids: Vec<String>,
}

pub struct AssignApplicationAccessUseCase<U: UnitOfWork> {
    principal_repo: Arc<PrincipalRepository>,
    application_repo: Arc<ApplicationRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> AssignApplicationAccessUseCase<U> {
    pub fn new(
        principal_repo: Arc<PrincipalRepository>,
        application_repo: Arc<ApplicationRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self { principal_repo, application_repo, unit_of_work }
    }

    pub async fn execute(
        &self,
        command: AssignApplicationAccessCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ApplicationAccessAssigned> {
        if command.user_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "USER_ID_REQUIRED", "User ID is required",
            ));
        }

        // Find the principal
        let mut principal = match self.principal_repo.find_by_id(&command.user_id).await {
            Ok(Some(p)) => p,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "USER_NOT_FOUND",
                    format!("User not found: {}", command.user_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch user: {}", e
                )));
            }
        };

        // Must be a USER type
        if principal.principal_type != PrincipalType::User {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "NOT_A_USER", "Principal is not a user",
            ));
        }

        // Validate all requested applications exist
        for app_id in &command.application_ids {
            match self.application_repo.find_by_id(app_id).await {
                Ok(Some(app)) => {
                    if !app.active {
                        return UseCaseResult::failure(UseCaseError::business_rule(
                            "APPLICATION_INACTIVE",
                            format!("Application is not active: {}", app_id),
                        ));
                    }
                }
                Ok(None) => {
                    return UseCaseResult::failure(UseCaseError::validation(
                        "APPLICATION_NOT_FOUND",
                        format!("Application not found: {}", app_id),
                    ));
                }
                Err(e) => {
                    return UseCaseResult::failure(UseCaseError::commit(format!(
                        "Failed to fetch application: {}", e
                    )));
                }
            }
        }

        // Compute delta
        let current: std::collections::HashSet<&str> = principal.accessible_application_ids
            .iter().map(|s| s.as_str()).collect();
        let requested: std::collections::HashSet<&str> = command.application_ids
            .iter().map(|s| s.as_str()).collect();

        let added: Vec<String> = requested.difference(&current)
            .map(|s| s.to_string()).collect();
        let removed: Vec<String> = current.difference(&requested)
            .map(|s| s.to_string()).collect();

        // Update principal
        principal.accessible_application_ids = command.application_ids.clone();
        principal.updated_at = chrono::Utc::now();

        let event = ApplicationAccessAssigned::new(
            &ctx,
            &principal.id,
            command.application_ids.clone(),
            added,
            removed,
        );

        self.unit_of_work.commit(&principal, event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = AssignApplicationAccessCommand {
            user_id: "user-123".to_string(),
            application_ids: vec!["app-1".to_string(), "app-2".to_string()],
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("userId"));
        assert!(json.contains("applicationIds"));
    }
}
