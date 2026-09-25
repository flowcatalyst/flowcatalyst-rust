//! Sync Roles Use Case
//!
//! Syncs roles from an application SDK. Creates new SDK-sourced roles,
//! updates existing SDK-sourced ones, and optionally removes unlisted
//! SDK-sourced roles. CODE and DATABASE-sourced roles are never modified.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::{RoleCreated, RoleDeleted, RoleUpdated, RolesSynced};
use crate::role::entity::{AuthRole, RoleSource};
use crate::usecase::{
    ExecutionContext, OrNotFound, RecordedEvent, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::ApplicationRepository;
use crate::RoleRepository;

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

impl crate::usecase::AuditMasked for SyncRolesCommand {}

pub struct SyncRolesUseCase<U: UnitOfWork> {
    role_repo: Arc<RoleRepository>,
    application_repo: Arc<crate::ApplicationRepository>,
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

    async fn authorize(
        &self,
        _command: &SyncRolesCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: SyncRolesCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<RolesSynced> {
        let (rows, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        // Go's usecaseop.Sync: a created/updated/deleted event per synced
        // role, then the rollup.
        self.unit_of_work.emit_events(rows, event, &command).await
    }
}

impl<U: UnitOfWork> SyncRolesUseCase<U> {
    async fn prepare(
        &self,
        command: &SyncRolesCommand,
        ctx: &ExecutionContext,
    ) -> Result<(Vec<RecordedEvent>, RolesSynced), UseCaseError> {
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
        let mut rows: Vec<RecordedEvent> = Vec::new();

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
                        if let Err(e) = self.role_repo.update(&updated).await {
                            return Err(UseCaseError::commit(format!(
                                "Failed to update role '{}': {}",
                                full_name, e
                            )));
                        }
                        rows.push(RecordedEvent::of(&RoleUpdated::new(
                            ctx,
                            &updated.id,
                            &updated.name,
                        ))?);
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
                    if let Err(e) = self.role_repo.insert(&role).await {
                        return Err(UseCaseError::commit(format!(
                            "Failed to create role '{}': {}",
                            full_name, e
                        )));
                    }
                    rows.push(RecordedEvent::of(&RoleCreated::new(
                        ctx, &role.id, &role.name,
                    ))?);
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
                    if let Err(e) = self.role_repo.delete(&role.id).await {
                        return Err(UseCaseError::commit(format!(
                            "Failed to delete role '{}': {}",
                            role.name, e
                        )));
                    }
                    rows.push(RecordedEvent::of(&RoleDeleted::new(
                        ctx, &role.id, &role.name,
                    ))?);
                    deleted_count += 1;
                }
            }
        }

        let event = RolesSynced {
            metadata: RolesSynced::metadata_for(ctx, &command.application_code),
            created: created_count,
            updated: updated_count,
            removed: deleted_count,
            total: command.roles.len() as u32,
            application_code: command.application_code.clone(),
            synced_codes: synced_names,
        };
        Ok((rows, event))
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
