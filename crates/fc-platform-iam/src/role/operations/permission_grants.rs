//! Per-role permission grants and the permission catalogue (Go
//! `role/operations/permissions.go`, `shared/bff/roles.go` createPermission,
//! `role/api/api.go` deletePermission).
//!
//! Grant and revoke address the role by name and are idempotent: a repeat
//! still saves and emits, so the audit trail records the admin action. They
//! apply to every role source (Go lets an operator grant on a CODE role).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::{RolePermissionGranted, RolePermissionRevoked};
use super::permission_events::{PermissionDefined, PermissionDeleted};
use crate::role::permission_catalog::CatalogPermission;
use crate::role::permission_repository::PermissionCatalogRepository;
use crate::role::repository::RoleRepository;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

fn require_names(role_name: &str, permission: &str) -> Result<(), UseCaseError> {
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

// ── Grant ────────────────────────────────────────────────────────────────

/// Go `GrantPermissionCommand`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantPermissionCommand {
    pub role_name: String,
    pub permission: String,
    /// Owner ruling 15: a super-admin may add another application's
    /// permission. Recorded in the audit row when set.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cross_application: bool,
}

impl fc_platform_core::usecase::AuditMasked for GrantPermissionCommand {}

pub struct GrantPermissionUseCase<U: UnitOfWork> {
    role_repo: Arc<RoleRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> GrantPermissionUseCase<U> {
    pub fn new(role_repo: Arc<RoleRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            role_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for GrantPermissionUseCase<U> {
    type Command = GrantPermissionCommand;
    type Event = RolePermissionGranted;

    async fn validate(&self, command: &GrantPermissionCommand) -> Result<(), UseCaseError> {
        require_names(&command.role_name, &command.permission)
    }

    /// The role ceiling (owner ruling 14): only a permission the caller holds may
    /// be granted, 403 `PERMISSION_ABOVE_CALLER`; granting one the role already
    /// holds changes nothing and is not bounded.
    async fn authorize(
        &self,
        command: &GrantPermissionCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        let already = self
            .role_repo
            .find_by_name(&command.role_name)
            .await?
            .is_some_and(|r| r.permissions.contains(&command.permission));
        if already {
            return Ok(());
        }
        Ok(crate::role::ceiling::require_permissions(
            Some(ctx.caller()),
            [command.permission.as_str()],
        )?)
    }

    async fn execute(
        &self,
        command: GrantPermissionCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<RolePermissionGranted>, UseCaseError> {
        let mut role = match self
            .role_repo
            .find_by_name(&command.role_name)
            .await
            .or_not_found(
                "ROLE_NOT_FOUND",
                format!("Role not found: {}", command.role_name),
            ) {
            Ok(r) => r,
            Err(e) => return Err(e),
        };
        if !role.permissions.contains(&command.permission) {
            super::require_confined(
                role.owning_application_code(),
                [command.permission.as_str()],
                command.cross_application,
            )?;
        }
        role.grant_permission(command.permission.clone());
        role.updated_at = chrono::Utc::now();
        let event = RolePermissionGranted::new(&ctx, &role.id, &role.name, &command.permission);
        self.unit_of_work
            .commit(&role, &*self.role_repo, event, &command)
            .await
    }
}

// ── Revoke ───────────────────────────────────────────────────────────────

/// Go `RevokePermissionCommand`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevokePermissionCommand {
    pub role_name: String,
    pub permission: String,
}

impl fc_platform_core::usecase::AuditMasked for RevokePermissionCommand {}

pub struct RevokePermissionUseCase<U: UnitOfWork> {
    role_repo: Arc<RoleRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> RevokePermissionUseCase<U> {
    pub fn new(role_repo: Arc<RoleRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            role_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for RevokePermissionUseCase<U> {
    type Command = RevokePermissionCommand;
    type Event = RolePermissionRevoked;

    async fn validate(&self, command: &RevokePermissionCommand) -> Result<(), UseCaseError> {
        require_names(&command.role_name, &command.permission)
    }

    /// The role ceiling (owner ruling 14): removal counts, so revoking a
    /// permission the role holds needs the caller to hold it, 403
    /// `PERMISSION_ABOVE_CALLER`; revoking an absent one is a no-op.
    async fn authorize(
        &self,
        command: &RevokePermissionCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        let held = self
            .role_repo
            .find_by_name(&command.role_name)
            .await?
            .is_some_and(|r| r.permissions.contains(&command.permission));
        if !held {
            return Ok(());
        }
        Ok(crate::role::ceiling::require_permissions(
            Some(ctx.caller()),
            [command.permission.as_str()],
        )?)
    }

    async fn execute(
        &self,
        command: RevokePermissionCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<RolePermissionRevoked>, UseCaseError> {
        let mut role = match self
            .role_repo
            .find_by_name(&command.role_name)
            .await
            .or_not_found(
                "ROLE_NOT_FOUND",
                format!("Role not found: {}", command.role_name),
            ) {
            Ok(r) => r,
            Err(e) => return Err(e),
        };
        role.revoke_permission(&command.permission);
        role.updated_at = chrono::Utc::now();
        let event = RolePermissionRevoked::new(&ctx, &role.id, &role.name, &command.permission);
        self.unit_of_work
            .commit(&role, &*self.role_repo, event, &command)
            .await
    }
}

// ── Catalogue: define ────────────────────────────────────────────────────

/// Define (or idempotently redefine) a catalogue permission from its four
/// segments (Go BFF `createPermission`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefinePermissionCommand {
    pub application: String,
    pub context: String,
    pub aggregate: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl fc_platform_core::usecase::AuditMasked for DefinePermissionCommand {}

impl DefinePermissionCommand {
    /// The canonical `application:context:aggregate:action`, segments trimmed.
    pub fn code(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.application.trim(),
            self.context.trim(),
            self.aggregate.trim(),
            self.action.trim()
        )
    }
}

/// Go `permSegment`: `^[a-z0-9-]+$`.
fn is_permission_segment(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub struct DefinePermissionUseCase<U: UnitOfWork> {
    repo: Arc<PermissionCatalogRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DefinePermissionUseCase<U> {
    pub fn new(repo: Arc<PermissionCatalogRepository>, unit_of_work: Arc<U>) -> Self {
        Self { repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DefinePermissionUseCase<U> {
    type Command = DefinePermissionCommand;
    type Event = PermissionDefined;

    async fn validate(&self, c: &DefinePermissionCommand) -> Result<(), UseCaseError> {
        for seg in [&c.application, &c.context, &c.aggregate, &c.action] {
            if !is_permission_segment(seg.trim()) {
                return Err(UseCaseError::validation(
                    "INVALID_PERMISSION",
                    "application, context, aggregate and action must each be lowercase letters, numbers or hyphens",
                ));
            }
        }
        Ok(())
    }

    /// The permission catalogue is an anchor's (`/api/roles/permissions` and the
    /// BFF's create both require anchor scope before the body is read).
    async fn authorize(
        &self,
        _command: &DefinePermissionCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(
            fc_platform_core::shared::authorization_service::checks::require_anchor_scope(
                ctx.caller(),
            )?,
        )
    }

    async fn execute(
        &self,
        command: DefinePermissionCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<PermissionDefined>, UseCaseError> {
        let code = command.code();
        let description = command
            .description
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .map(str::to_string);
        let existing = self.repo.find_by_code(&code).await?;
        let permission = match existing {
            Some(mut p) => {
                p.description = description;
                p.updated_at = chrono::Utc::now();
                p
            }
            None => match CatalogPermission::new(&code, description) {
                Some(p) => p,
                None => {
                    return Err(UseCaseError::validation(
                        "INVALID_PERMISSION",
                        "a permission is application:context:aggregate:action",
                    ))
                }
            },
        };
        let event = PermissionDefined::new(
            &ctx,
            &permission.id,
            &permission.code,
            permission.description.as_deref(),
        );
        self.unit_of_work
            .commit(&permission, &*self.repo, event, &command)
            .await
    }
}

// ── Catalogue: delete ────────────────────────────────────────────────────

/// Delete a catalogue permission by code (Go `deletePermission`). The
/// handler answers 204 for an absent code without running this.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletePermissionCommand {
    pub permission: String,
}

impl fc_platform_core::usecase::AuditMasked for DeletePermissionCommand {}

pub struct DeletePermissionUseCase<U: UnitOfWork> {
    repo: Arc<PermissionCatalogRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeletePermissionUseCase<U> {
    pub fn new(repo: Arc<PermissionCatalogRepository>, unit_of_work: Arc<U>) -> Self {
        Self { repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeletePermissionUseCase<U> {
    type Command = DeletePermissionCommand;
    type Event = PermissionDeleted;

    async fn validate(&self, c: &DeletePermissionCommand) -> Result<(), UseCaseError> {
        if c.permission.trim().is_empty() {
            return Err(UseCaseError::validation(
                "PERMISSION_REQUIRED",
                "Permission is required",
            ));
        }
        Ok(())
    }

    /// The permission catalogue is an anchor's (`/api/roles/permissions` and the
    /// BFF's create both require anchor scope before the body is read).
    async fn authorize(
        &self,
        _command: &DeletePermissionCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(
            fc_platform_core::shared::authorization_service::checks::require_anchor_scope(
                ctx.caller(),
            )?,
        )
    }

    async fn execute(
        &self,
        command: DeletePermissionCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<PermissionDeleted>, UseCaseError> {
        let permission = match self
            .repo
            .find_by_code(&command.permission)
            .await
            .or_not_found(
                "PERMISSION_NOT_FOUND",
                format!("Permission not found: {}", command.permission),
            ) {
            Ok(p) => p,
            Err(e) => return Err(e),
        };
        let event = PermissionDeleted::new(&ctx, &permission.id, &permission.code);
        self.unit_of_work
            .commit_delete(&permission, &*self.repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_are_lowercase_tokens() {
        assert!(is_permission_segment("order-line2"));
        assert!(!is_permission_segment("Order"));
        assert!(!is_permission_segment(""));
        assert!(!is_permission_segment("a:b"));
    }
}
