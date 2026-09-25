//! Grant / Revoke Role Permission Use Cases
//!
//! Go's `GrantPermission` / `RevokePermission`
//! (`role/operations/permissions.go`): add or remove one permission on the
//! role named by `roleName` and emit `platform:admin:role:permission-granted`
//! / `:permission-revoked` `{roleId, roleName, permission}`. Idempotent, as
//! Go: re-granting a held permission (or revoking an absent one) still
//! commits and emits, so the audit trail records the admin action.
//!
//! The platform's own rules stay: only a DATABASE role is modified, and a
//! granted permission must be the role's application's own (owner ruling
//! 15) unless `cross_application` is set by a super-admin caller. The
//! caller's permission ceiling (ruling 14) is checked by the handler.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::{RolePermissionGranted, RolePermissionRevoked};
use crate::role::entity::{AuthRole, RoleSource};
use crate::role::repository::RoleRepository;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

/// Grant `permission` on the role named `role_name`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantRolePermissionCommand {
    pub role_name: String,
    pub permission: String,
    /// May the permission be another application's? Only a super-admin
    /// caller sets this (owner ruling 15); recorded in the audit row.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cross_application: bool,
}

impl crate::usecase::AuditMasked for GrantRolePermissionCommand {}

/// Revoke `permission` from the role named `role_name`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevokeRolePermissionCommand {
    pub role_name: String,
    pub permission: String,
}

impl crate::usecase::AuditMasked for RevokeRolePermissionCommand {}

fn require_fields(role_name: &str, permission: &str) -> Result<(), UseCaseError> {
    if role_name.trim().is_empty() {
        return Err(UseCaseError::validation(
            "ROLE_NAME_REQUIRED",
            "Role name is required",
        ));
    }
    if permission.trim().is_empty() {
        return Err(UseCaseError::validation(
            "PERMISSION_REQUIRED",
            "Permission is required",
        ));
    }
    Ok(())
}

async fn load_modifiable(repo: &RoleRepository, role_name: &str) -> Result<AuthRole, UseCaseError> {
    let role = repo
        .find_by_name(role_name)
        .await
        .or_not_found("ROLE_NOT_FOUND", format!("Role '{}' not found", role_name))?;
    if role.source != RoleSource::Database {
        return Err(UseCaseError::business_rule(
            "CANNOT_MODIFY_ROLE",
            "Cannot modify a code-defined or SDK-synced role",
        ));
    }
    Ok(role)
}

/// Use case for granting one permission on a role.
pub struct GrantRolePermissionUseCase<U: UnitOfWork> {
    role_repo: Arc<RoleRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> GrantRolePermissionUseCase<U> {
    pub fn new(role_repo: Arc<RoleRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            role_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for GrantRolePermissionUseCase<U> {
    type Command = GrantRolePermissionCommand;
    type Event = RolePermissionGranted;

    async fn validate(&self, command: &GrantRolePermissionCommand) -> Result<(), UseCaseError> {
        require_fields(&command.role_name, &command.permission)
    }

    async fn authorize(
        &self,
        _command: &GrantRolePermissionCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: GrantRolePermissionCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<RolePermissionGranted> {
        let mut role = match load_modifiable(&self.role_repo, &command.role_name).await {
            Ok(r) => r,
            Err(e) => return UseCaseResult::failure(e),
        };
        if !role.permissions.contains(&command.permission) {
            if let Err(e) = super::require_confined(
                role.owning_application_code(),
                [command.permission.as_str()],
                command.cross_application,
            ) {
                return UseCaseResult::failure(e);
            }
        }
        role.grant_permission(command.permission.clone());
        role.updated_at = chrono::Utc::now();
        let event = RolePermissionGranted::new(&ctx, &role.id, &role.name, &command.permission);
        self.unit_of_work
            .commit(&role, &*self.role_repo, event, &command)
            .await
    }
}

/// Use case for revoking one permission from a role.
pub struct RevokeRolePermissionUseCase<U: UnitOfWork> {
    role_repo: Arc<RoleRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> RevokeRolePermissionUseCase<U> {
    pub fn new(role_repo: Arc<RoleRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            role_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for RevokeRolePermissionUseCase<U> {
    type Command = RevokeRolePermissionCommand;
    type Event = RolePermissionRevoked;

    async fn validate(&self, command: &RevokeRolePermissionCommand) -> Result<(), UseCaseError> {
        require_fields(&command.role_name, &command.permission)
    }

    async fn authorize(
        &self,
        _command: &RevokeRolePermissionCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: RevokeRolePermissionCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<RolePermissionRevoked> {
        let mut role = match load_modifiable(&self.role_repo, &command.role_name).await {
            Ok(r) => r,
            Err(e) => return UseCaseResult::failure(e),
        };
        role.revoke_permission(&command.permission);
        role.updated_at = chrono::Utc::now();
        let event = RolePermissionRevoked::new(&ctx, &role.id, &role.name, &command.permission);
        self.unit_of_work
            .commit(&role, &*self.role_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_go_shaped() {
        let ctx = ExecutionContext::create("prn_1");
        let e = RolePermissionGranted::new(&ctx, "rol_1", "orders:viewer", "orders:order:read");
        assert_eq!(
            e.metadata.event_type,
            "platform:admin:role:permission-granted"
        );
        assert_eq!(e.metadata.subject, "platform.role.rol_1");
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({
                "roleId": "rol_1",
                "roleName": "orders:viewer",
                "permission": "orders:order:read"
            })
        );
    }

    #[test]
    fn blank_fields_are_refused_with_go_codes() {
        assert_eq!(
            require_fields(" ", "p").unwrap_err().code(),
            "ROLE_NAME_REQUIRED"
        );
        assert_eq!(
            require_fields("r", "").unwrap_err().code(),
            "PERMISSION_REQUIRED"
        );
    }
}
