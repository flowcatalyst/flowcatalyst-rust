//! Delete Role Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::RoleDeleted;
use crate::role::entity::{AuthRole, RoleSource};
use crate::role::repository::RoleRepository;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

/// Command for deleting a role.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteRoleCommand {
    /// Role ID to delete
    pub role_id: String,
}

impl crate::usecase::AuditMasked for DeleteRoleCommand {}

/// Use case for deleting a role.
pub struct DeleteRoleUseCase<U: UnitOfWork> {
    role_repo: Arc<RoleRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteRoleUseCase<U> {
    pub fn new(role_repo: Arc<RoleRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            role_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteRoleUseCase<U> {
    type Command = DeleteRoleCommand;
    type Event = RoleDeleted;

    async fn validate(&self, command: &DeleteRoleCommand) -> Result<(), UseCaseError> {
        if command.role_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "ROLE_ID_REQUIRED",
                "Role ID is required",
            ));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &DeleteRoleCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: DeleteRoleCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<RoleDeleted> {
        let (role, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        // Atomic commit with delete
        self.unit_of_work
            .commit_delete(&role, &*self.role_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> DeleteRoleUseCase<U> {
    async fn prepare(
        &self,
        command: &DeleteRoleCommand,
        ctx: &ExecutionContext,
    ) -> Result<(AuthRole, RoleDeleted), UseCaseError> {
        // Fetch existing role
        let role = self
            .role_repo
            .find_by_id(&command.role_id)
            .await
            .or_not_found(
                "ROLE_NOT_FOUND",
                format!("Role with ID '{}' not found", command.role_id),
            )?;

        // Business rule: a code-defined role is immutable (Go delete.go:
        // 409 `CODE_ROLE_IMMUTABLE`; database and SDK roles may go).
        if role.source == RoleSource::Code {
            return Err(UseCaseError::business_rule(
                "CODE_ROLE_IMMUTABLE",
                "Roles with source=CODE cannot be deleted",
            ));
        }

        // Business rule: refuse when principals still hold this role.
        // iam_principal_roles has no DB-level FK on role_name (integrity is
        // enforced in code), so dropping the role here would orphan the
        // assignments. Force the admin to strip assignments first.
        let assignments = self.role_repo.count_assignments(&role.name).await?;
        if assignments > 0 {
            return Err(UseCaseError::business_rule(
                "ROLE_HAS_ASSIGNMENTS",
                format!(
                    "Cannot delete role '{}' — {} principal(s) still hold it. \
                     Strip the assignments before deleting.",
                    role.name, assignments,
                ),
            ));
        }

        // Create domain event
        let event = RoleDeleted::new(ctx, &role.id, &role.name);
        Ok((role, event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = DeleteRoleCommand {
            role_id: "role-123".to_string(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("roleId"));
    }
}
