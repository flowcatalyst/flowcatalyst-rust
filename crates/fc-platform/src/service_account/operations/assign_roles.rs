//! Assign Roles to Service Account Use Case

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

use super::events::ServiceAccountRolesAssigned;
use crate::service_account::entity::{AssignmentSource, RoleAssignment};
use crate::service_account::ServiceAccount;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::ServiceAccountRepository;

/// Command for assigning roles to a service account (declarative - replaces all).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignRolesCommand {
    /// Service account ID
    pub service_account_id: String,

    /// Role names to assign (replaces existing roles)
    pub roles: Vec<String>,
}

impl crate::usecase::AuditMasked for AssignRolesCommand {}

/// Use case for assigning roles to a service account.
pub struct AssignRolesUseCase<U: UnitOfWork> {
    service_account_repo: Arc<ServiceAccountRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> AssignRolesUseCase<U> {
    pub fn new(service_account_repo: Arc<ServiceAccountRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            service_account_repo,
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

    async fn authorize(
        &self,
        _command: &AssignRolesCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: AssignRolesCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ServiceAccountRolesAssigned> {
        let (service_account, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

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

impl<U: UnitOfWork> AssignRolesUseCase<U> {
    async fn prepare(
        &self,
        command: &AssignRolesCommand,
        ctx: &ExecutionContext,
    ) -> Result<(ServiceAccount, ServiceAccountRolesAssigned), UseCaseError> {
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

        // Replace roles
        service_account.roles = command
            .roles
            .iter()
            .map(|r| RoleAssignment::with_source(r, AssignmentSource::AdminAssigned))
            .collect();
        service_account.updated_at = Utc::now();

        // Create domain event
        let event =
            ServiceAccountRolesAssigned::new(ctx, &service_account.id, roles_added, roles_removed);
        Ok((service_account, event))
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
