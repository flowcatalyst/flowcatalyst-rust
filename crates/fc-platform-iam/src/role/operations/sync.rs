//! Sync Roles Use Case
//!
//! Syncs roles from an application SDK. Creates new SDK-sourced roles,
//! updates existing SDK-sourced ones, and optionally removes unlisted
//! SDK-sourced roles. CODE and DATABASE-sourced roles are never modified.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::{RoleCreated, RoleDeleted, RoleUpdated, RolesSynced};
use crate::application::repository::ApplicationRepository;
use crate::role::entity::{AuthRole, RoleSource};
use crate::role::repository::RoleRepository;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, RecordedEvent, UnitOfWork, UseCase, UseCaseError,
};

/// A single role definition in the sync payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRoleInput {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub client_managed: bool,
}

/// Command for syncing roles from an application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRolesCommand {
    pub application_code: String,
    pub roles: Vec<SyncRoleInput>,
    #[serde(default)]
    pub remove_unlisted: bool,
}

impl AuditMasked for SyncRolesCommand {}

pub struct SyncRolesUseCase<U: UnitOfWork> {
    role_repo: Arc<RoleRepository>,
    application_repo: Arc<ApplicationRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> SyncRolesUseCase<U> {
    pub fn new(
        role_repo: Arc<RoleRepository>,
        application_repo: Arc<ApplicationRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            role_repo,
            application_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for SyncRolesUseCase<U> {
    type Command = SyncRolesCommand;
    type Event = RolesSynced;

    async fn validate(&self, command: &SyncRolesCommand) -> Result<(), UseCaseError> {
        if command.application_code.trim().is_empty() {
            return Err(UseCaseError::validation(
                "APPLICATION_CODE_REQUIRED",
                "Application code is required",
            ));
        }

        if command.roles.is_empty() {
            return Err(UseCaseError::validation(
                "ROLES_REQUIRED",
                "At least one role must be provided",
            ));
        }

        Ok(())
    }

    /// The application must be in the caller's application scope (the SDK
    /// handler resolves `/{appCode}` against it first, answering the same 404,
    /// and attaches the scope). A missing application is `execute`'s.
    async fn authorize(
        &self,
        command: &SyncRolesCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        if let Some(app) = self
            .application_repo
            .find_by_code(&command.application_code)
            .await?
        {
            checks::require_caller_application_access(
                ctx.caller(),
                &command.application_code,
                Some(app),
            )
            .map_err(UseCaseError::verbatim)?;
        }
        Ok(())
    }

    async fn execute(
        &self,
        command: SyncRolesCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<RolesSynced>, UseCaseError> {
        // Verify the application exists
        let application = self
            .application_repo
            .find_by_code(&command.application_code)
            .await
            .or_not_found(
                "APPLICATION_NOT_FOUND",
                format!("Application not found: {}", command.application_code),
            )?;

        // Fetch existing roles for this application
        let existing = self
            .role_repo
            .find_by_application(&command.application_code)
            .await?;

        // Owner ruling 15: an application's roles hold only its own
        // permissions, and the SDK sync never gets the super-admin exception.
        for input in &command.roles {
            super::require_confined(
                &command.application_code,
                input.permissions.iter().map(String::as_str),
                false,
            )?;
        }

        let mut created_count = 0u32;
        let mut updated_count = 0u32;
        let mut deleted_count = 0u32;
        let mut synced_names: Vec<String> = Vec::new();
        let mut saves: Vec<AuthRole> = Vec::new();
        let mut deletes: Vec<AuthRole> = Vec::new();
        let mut rows: Vec<RecordedEvent> = Vec::new();

        // Plan every row before anything is written (Go's `usecaseop.Sync`):
        // a refused row fails the sync with nothing written.

        for input in &command.roles {
            // As Go (`splitRoleName`): a name that already carries this
            // application's prefix is not prefixed twice.
            let short_name = short_role_name(&input.name.to_lowercase(), &command.application_code);
            let full_name = format!("{}:{}", command.application_code, short_name);
            synced_names.push(full_name.clone());

            let existing_role = existing.iter().find(|r| r.name == full_name);
            match existing_role {
                Some(role) => {
                    // Only update SDK-sourced roles
                    if role.source == RoleSource::Sdk {
                        let mut updated = role.clone();
                        updated.display_name = input
                            .display_name
                            .clone()
                            .unwrap_or_else(|| input.name.clone());
                        updated.description = input.description.clone();
                        // As Go: apps usually declare role names and curate
                        // permissions in the UI, so an empty list keeps the
                        // stored permissions; only a non-empty list replaces them.
                        if !input.permissions.is_empty() {
                            updated.permissions = input.permissions.iter().cloned().collect();
                        }
                        updated.client_managed = input.client_managed;
                        updated.updated_at = chrono::Utc::now();
                        rows.push(RecordedEvent::of(&RoleUpdated::new(
                            &ctx,
                            &updated.id,
                            &updated.name,
                        ))?);
                        saves.push(updated);
                        updated_count += 1;
                    }
                    // Skip CODE and DATABASE-sourced roles
                }
                None => {
                    let mut role = AuthRole::new(
                        &command.application_code,
                        short_name.clone(),
                        input.display_name.as_deref().unwrap_or(&input.name),
                    );
                    role.application_id = Some(application.id.clone());
                    role.source = RoleSource::Sdk;
                    role.description = input.description.clone();
                    role.permissions = input.permissions.iter().cloned().collect();
                    role.client_managed = input.client_managed;
                    rows.push(RecordedEvent::of(&RoleCreated::new(
                        &ctx, &role.id, &role.name,
                    ))?);
                    saves.push(role);
                    created_count += 1;
                }
            }
        }

        // Remove unlisted SDK-sourced roles for this application.
        // Refuse if a role still has principal assignments — the junction
        // has no DB-level FK, so silently dropping it would orphan user
        // role assignments. The caller (SDK sync command) must strip the
        // assignments first.
        if command.remove_unlisted {
            for role in &existing {
                if role.source == RoleSource::Sdk && !synced_names.contains(&role.name) {
                    let assignments = self.role_repo.count_assignments(&role.name).await?;
                    if assignments > 0 {
                        return Err(UseCaseError::business_rule(
                            "ROLE_HAS_ASSIGNMENTS",
                            format!(
                                "Cannot remove role '{}' — {} principal(s) still hold it. \
                                 Strip the assignments before syncing.",
                                role.name, assignments,
                            ),
                        ));
                    }
                    rows.push(RecordedEvent::of(&RoleDeleted::new(
                        &ctx, &role.id, &role.name,
                    ))?);
                    deletes.push(role.clone());
                    deleted_count += 1;
                }
            }
        }

        let event = RolesSynced {
            metadata: RolesSynced::metadata_for(&ctx, &command.application_code),
            created: created_count,
            updated: updated_count,
            removed: deleted_count,
            total: command.roles.len() as u32,
            application_code: command.application_code.clone(),
            synced_codes: synced_names,
        };

        // Go's usecaseop.Sync: the rows, a created/updated/deleted event per
        // synced role, then the rollup, in one transaction.
        self.unit_of_work
            .commit_sync(&*self.role_repo, &saves, &deletes, rows, event, &command)
            .await
    }
}

/// The short role name: `name` without a leading `{application_code}:` (Go
/// `splitRoleName`), so `orders:admin` and `admin` name the same role.
fn short_role_name(name: &str, application_code: &str) -> String {
    let prefix = format!("{application_code}:");
    match name.strip_prefix(&prefix) {
        Some(short) if !short.is_empty() => short.to_string(),
        _ => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = SyncRolesCommand {
            application_code: "orders".to_string(),
            roles: vec![SyncRoleInput {
                name: "admin".to_string(),
                display_name: Some("Orders Admin".to_string()),
                description: None,
                permissions: vec!["orders:read".to_string()],
                client_managed: false,
            }],
            remove_unlisted: false,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("orders"));
    }
}
