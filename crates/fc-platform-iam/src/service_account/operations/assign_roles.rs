//! Assign Roles to Service Account Use Case

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

use super::events::ServiceAccountRolesAssigned;
use crate::role::ceiling;
use crate::role::repository::RoleRepository;
use crate::service_account::entity::{AssignmentSource, RoleAssignment};
use crate::service_account::repository::ServiceAccountRepository;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::shared::error::PlatformError;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for assigning roles to a service account (declarative - replaces all).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignRolesCommand {
    /// Service account ID
    pub service_account_id: String,

    /// Role names to assign (replaces existing roles)
    pub roles: Vec<String>,
}

impl AuditMasked for AssignRolesCommand {}

/// Use case for assigning roles to a service account.
pub struct AssignRolesUseCase<U: UnitOfWork> {
    service_account_repo: Arc<ServiceAccountRepository>,
    /// The role ceiling's definitions (owner ruling 14).
    role_repo: Arc<RoleRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> AssignRolesUseCase<U> {
    pub fn new(
        service_account_repo: Arc<ServiceAccountRepository>,
        role_repo: Arc<RoleRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            service_account_repo,
            role_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for AssignRolesUseCase<U> {
    type Command = AssignRolesCommand;
    type Event = ServiceAccountRolesAssigned;

    async fn validate(&self, _command: &AssignRolesCommand) -> Result<(), UseCaseError> {
        Ok(())
    }

    /// Anchors only (`can_update_service_accounts`, checked with the permission
    /// by the handler before the body): an application's own ANCHOR-tier account
    /// could otherwise grant itself super-admin. Then the role ceiling (owner
    /// ruling 14): only roles whose every permission the caller holds may be
    /// added or removed, 403 `ROLE_ABOVE_CALLER`. A missing account is the
    /// handler's `ServiceAccount_NOT_FOUND`, answered before the ceiling.
    async fn authorize(
        &self,
        command: &AssignRolesCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        checks::require_anchor_scope(ctx.caller())?;
        let account = self
            .service_account_repo
            .find_by_id(&command.service_account_id)
            .await?
            .ok_or_else(|| {
                UseCaseError::verbatim(PlatformError::ServiceAccountNotFound {
                    id: command.service_account_id.clone(),
                })
            })?;
        let before: Vec<String> = account.roles.iter().map(|r| r.role.clone()).collect();
        ceiling::require_role_change(ctx.caller(), &self.role_repo, &before, &command.roles).await
    }

    async fn execute(
        &self,
        command: AssignRolesCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<ServiceAccountRolesAssigned>, UseCaseError> {
        // Find the service account
        let mut service_account = self
            .service_account_repo
            .find_by_id(&command.service_account_id)
            .await
            .or_not_found(
                "SERVICE_ACCOUNT_NOT_FOUND",
                format!(
                    "Service account with ID '{}' not found",
                    command.service_account_id
                ),
            )?;

        // Calculate diff
        let current_roles: HashSet<String> = service_account
            .roles
            .iter()
            .map(|r| r.role.clone())
            .collect();
        let new_roles: HashSet<String> = command.roles.iter().cloned().collect();

        let roles_added: Vec<String> = new_roles.difference(&current_roles).cloned().collect();
        let roles_removed: Vec<String> = current_roles.difference(&new_roles).cloned().collect();

        // Replace roles, as Go does (every assignment is made now), recording
        // who made them.
        service_account.roles = command
            .roles
            .iter()
            .map(|r| {
                RoleAssignment::with_source(r, AssignmentSource::AdminAssigned)
                    .assigned_by(&ctx.principal_id)
            })
            .collect();
        service_account.updated_at = Utc::now();

        // Create domain event
        let event =
            ServiceAccountRolesAssigned::new(&ctx, &service_account, roles_added, roles_removed);

        // Atomic commit
        self.unit_of_work
            .commit(
                &service_account,
                &*self.service_account_repo,
                event,
                &command,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = AssignRolesCommand {
            service_account_id: "sa-123".to_string(),
            roles: vec!["ADMIN".to_string(), "VIEWER".to_string()],
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("sa-123"));
        assert!(json.contains("ADMIN"));
    }
}
