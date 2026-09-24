//! Assign Roles Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::RolesAssigned;
use crate::principal::entity::{Principal, PrincipalType};
use crate::service_account::entity::{AssignmentSource, RoleAssignment};
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::PrincipalRepository;
use crate::RoleRepository;

/// Command for assigning roles to a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignUserRolesCommand {
    pub user_id: String,
    pub roles: Vec<String>,
}

impl crate::usecase::AuditMasked for AssignUserRolesCommand {}

pub struct AssignUserRolesUseCase<U: UnitOfWork> {
    principal_repo: Arc<PrincipalRepository>,
    role_repo: Arc<RoleRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> AssignUserRolesUseCase<U> {
    pub fn new(
        principal_repo: Arc<PrincipalRepository>,
        role_repo: Arc<RoleRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            principal_repo,
            role_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for AssignUserRolesUseCase<U> {
    type Command = AssignUserRolesCommand;
    type Event = RolesAssigned;

    async fn validate(&self, command: &AssignUserRolesCommand) -> Result<(), UseCaseError> {
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
        _command: &AssignUserRolesCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: AssignUserRolesCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<RolesAssigned> {
        let (principal, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit(&principal, &*self.principal_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> AssignUserRolesUseCase<U> {
    async fn prepare(
        &self,
        command: &AssignUserRolesCommand,
        ctx: &ExecutionContext,
    ) -> Result<(Principal, RolesAssigned), UseCaseError> {
        let mut principal = self
            .principal_repo
            .find_by_id(&command.user_id)
            .await
            .or_not_found(
                "USER_NOT_FOUND",
                format!("User with ID '{}' not found", command.user_id),
            )?;

        // Must be a USER type principal
        if principal.principal_type != PrincipalType::User {
            return Err(UseCaseError::business_rule(
                "NOT_A_USER",
                "Roles can only be assigned to USER type principals",
            ));
        }

        // Validate all roles exist. A bad role name is a client-body error
        // (400), not a 404 — this endpoint's 404 is reserved for "principal
        // not found".
        for role_name in &command.roles {
            if !self.role_repo.exists_by_name(role_name).await? {
                return Err(UseCaseError::validation(
                    "ROLE_NOT_FOUND",
                    format!("Role '{}' not found", role_name),
                ));
            }
        }

        // Compute delta
        let previous_roles: Vec<String> = principal.roles.iter().map(|r| r.role.clone()).collect();
        let added: Vec<String> = command
            .roles
            .iter()
            .filter(|r| !previous_roles.contains(r))
            .cloned()
            .collect();
        let removed: Vec<String> = previous_roles
            .iter()
            .filter(|r| !command.roles.contains(r))
            .cloned()
            .collect();

        // Replace roles with new assignments
        principal.roles = command
            .roles
            .iter()
            .map(|r| RoleAssignment::with_source(r, AssignmentSource::AdminAssigned))
            .collect();
        principal.updated_at = chrono::Utc::now();

        let event = RolesAssigned::new(ctx, &principal.id, command.roles.clone(), added, removed);
        Ok((principal, event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = AssignUserRolesCommand {
            user_id: "user-123".to_string(),
            roles: vec!["admin".to_string(), "viewer".to_string()],
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("userId"));
        assert!(json.contains("admin"));
    }
}
