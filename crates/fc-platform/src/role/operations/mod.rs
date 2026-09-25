//! Role Operations
//!
//! Use cases for role management.

pub mod create;
pub mod delete;
pub mod events;
pub mod permissions;
pub mod sync;
pub mod update;

pub use create::{CreateRoleCommand, CreateRoleUseCase};
pub use delete::{DeleteRoleCommand, DeleteRoleUseCase};
pub use events::*;
pub use permissions::{
    GrantRolePermissionCommand, GrantRolePermissionUseCase, RevokeRolePermissionCommand,
    RevokeRolePermissionUseCase,
};
pub use sync::{SyncRoleInput, SyncRolesCommand, SyncRolesUseCase};
pub use update::{UpdateRoleCommand, UpdateRoleUseCase};

use crate::usecase::UseCaseError;

/// Owner ruling 15 (Java a120e236): a role may hold only its own
/// application's permissions, 400 `PERMISSION_OUTSIDE_APPLICATION`
/// otherwise. `cross_application` lifts the rule; only the admin API sets
/// it, for a super-admin caller. The SDK role routes and the SDK sync never
/// do. Checked on what a write adds, never retroactively.
pub(crate) fn require_confined<'a>(
    application_code: &str,
    permissions: impl IntoIterator<Item = &'a str>,
    cross_application: bool,
) -> Result<(), UseCaseError> {
    let outside =
        crate::role::entity::permissions_outside_application(application_code, permissions);
    if outside.is_empty() {
        return Ok(());
    }
    if cross_application {
        // On record: the audit row carries the command, which names them.
        tracing::info!(
            application = application_code,
            permissions = ?outside,
            "super-admin put another application's permissions on a role"
        );
        return Ok(());
    }
    Err(UseCaseError::validation(
        "PERMISSION_OUTSIDE_APPLICATION",
        format!(
            "Permission '{}' does not belong to application '{}'; a role may only hold its own application's permissions",
            outside[0], application_code
        ),
    ))
}
