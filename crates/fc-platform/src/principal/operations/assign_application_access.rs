//! Assign Application Access Use Case
//!
//! Sets which applications a user or service account can access.
//! Computes delta (added/removed) and persists via UnitOfWork.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ApplicationAccessAssigned;
use crate::principal::entity::Principal;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::ApplicationRepository;
use crate::PrincipalRepository;

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
        Self {
            principal_repo,
            application_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for AssignApplicationAccessUseCase<U> {
    type Command = AssignApplicationAccessCommand;
    type Event = ApplicationAccessAssigned;

    async fn validate(&self, command: &AssignApplicationAccessCommand) -> Result<(), UseCaseError> {
        if command.user_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "USER_ID_REQUIRED",
                "User ID is required",
            ));
        }

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &AssignApplicationAccessCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: AssignApplicationAccessCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ApplicationAccessAssigned> {
        let (principal, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit(&principal, &*self.principal_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> AssignApplicationAccessUseCase<U> {
    async fn prepare(
        &self,
        command: &AssignApplicationAccessCommand,
        ctx: &ExecutionContext,
    ) -> Result<(Principal, ApplicationAccessAssigned), UseCaseError> {
        // Find the principal
        let mut principal = self
            .principal_repo
            .find_by_id(&command.user_id)
            .await
            .or_not_found(
                "USER_NOT_FOUND",
                format!("User not found: {}", command.user_id),
            )?;

        // Users and service accounts both carry application access (Go
        // assigns to both); a new service account has none until it is
        // granted here.

        // Validate all requested applications exist
        for app_id in &command.application_ids {
            match self.application_repo.find_by_id(app_id).await? {
                Some(app) => {
                    if !app.active {
                        return Err(UseCaseError::business_rule(
                            "APPLICATION_INACTIVE",
                            format!("Application is not active: {}", app_id),
                        ));
                    }
                }
                None => {
                    return Err(UseCaseError::validation(
                        "APPLICATION_NOT_FOUND",
                        format!("Application not found: {}", app_id),
                    ));
                }
            }
        }

        // Compute delta
        let current: std::collections::HashSet<&str> = principal
            .accessible_application_ids
            .iter()
            .map(|s| s.as_str())
            .collect();
        let requested: std::collections::HashSet<&str> =
            command.application_ids.iter().map(|s| s.as_str()).collect();

        let added: Vec<String> = requested
            .difference(&current)
            .map(|s| s.to_string())
            .collect();
        let removed: Vec<String> = current
            .difference(&requested)
            .map(|s| s.to_string())
            .collect();

        // Update principal
        principal.accessible_application_ids = command.application_ids.clone();
        principal.updated_at = chrono::Utc::now();

        let event = ApplicationAccessAssigned::new(
            ctx,
            &principal.id,
            command.application_ids.clone(),
            added,
            removed,
        );
        Ok((principal, event))
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
