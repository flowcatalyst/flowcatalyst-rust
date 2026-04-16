//! Sync Roles Use Case
//!
//! Syncs roles from an application SDK. Creates new SDK-sourced roles,
//! updates existing SDK-sourced ones, and optionally removes unlisted
//! SDK-sourced roles. CODE and DATABASE-sourced roles are never modified.

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::role::entity::{AuthRole, RoleSource};
use crate::RoleRepository;
use crate::ApplicationRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use super::events::RolesSynced;

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
        Self { role_repo, application_repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for SyncRolesUseCase<U> {
    type Command = SyncRolesCommand;
    type Event = RolesSynced;

    async fn validate(&self, command: &SyncRolesCommand) -> Result<(), UseCaseError> {
        if command.application_code.trim().is_empty() {
            return Err(UseCaseError::validation(
                "APPLICATION_CODE_REQUIRED", "Application code is required",
            ));
        }

        if command.roles.is_empty() {
            return Err(UseCaseError::validation(
                "ROLES_REQUIRED", "At least one role must be provided",
            ));
        }

        Ok(())
    }

    async fn authorize(&self, _command: &SyncRolesCommand, _ctx: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: SyncRolesCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<RolesSynced> {
        // Verify the application exists
        let application = match self.application_repo.find_by_code(&command.application_code).await {
            Ok(Some(app)) => app,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "APPLICATION_NOT_FOUND",
                    format!("Application not found: {}", command.application_code),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch application: {}", e
                )));
            }
        };

        // Fetch existing roles for this application
        let existing = match self.role_repo.find_by_application(&command.application_code).await {
            Ok(list) => list,
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch existing roles: {}", e
                )));
            }
        };

        let mut created_count = 0u32;
        let mut updated_count = 0u32;
        let mut deleted_count = 0u32;
        let mut synced_names: Vec<String> = Vec::new();

        for input in &command.roles {
            let full_name = format!("{}:{}", command.application_code, input.name.to_lowercase());
            synced_names.push(full_name.clone());

            let existing_role = existing.iter().find(|r| r.name == full_name);
            match existing_role {
                Some(role) => {
                    // Only update SDK-sourced roles
                    if role.source == RoleSource::Sdk {
                        let mut updated = role.clone();
                        updated.display_name = input.display_name.clone()
                            .unwrap_or_else(|| input.name.clone());
                        updated.description = input.description.clone();
                        updated.permissions = input.permissions.iter().cloned().collect();
                        updated.client_managed = input.client_managed;
                        updated.updated_at = chrono::Utc::now();
                        if let Err(e) = self.role_repo.update(&updated).await {
                            return UseCaseResult::failure(UseCaseError::commit(format!(
                                "Failed to update role '{}': {}", full_name, e
                            )));
                        }
                        updated_count += 1;
                    }
                    // Skip CODE and DATABASE-sourced roles
                }
                None => {
                    let mut role = AuthRole::new(
                        &command.application_code,
                        &input.name.to_lowercase(),
                        input.display_name.as_deref().unwrap_or(&input.name),
                    );
                    role.application_id = Some(application.id.clone());
                    role.source = RoleSource::Sdk;
                    role.description = input.description.clone();
                    role.permissions = input.permissions.iter().cloned().collect();
                    role.client_managed = input.client_managed;
                    if let Err(e) = self.role_repo.insert(&role).await {
                        return UseCaseResult::failure(UseCaseError::commit(format!(
                            "Failed to create role '{}': {}", full_name, e
                        )));
                    }
                    created_count += 1;
                }
            }
        }

        // Remove unlisted SDK-sourced roles for this application
        if command.remove_unlisted {
            for role in &existing {
                if role.source == RoleSource::Sdk && !synced_names.contains(&role.name) {
                    if let Err(e) = self.role_repo.delete(&role.id).await {
                        return UseCaseResult::failure(UseCaseError::commit(format!(
                            "Failed to delete role '{}': {}", role.name, e
                        )));
                    }
                    deleted_count += 1;
                }
            }
        }

        let event = RolesSynced::new(
            &ctx,
            &command.application_code,
            created_count,
            updated_count,
            deleted_count,
            synced_names,
        );

        self.unit_of_work.emit_event(event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = SyncRolesCommand {
            application_code: "orders".to_string(),
            roles: vec![
                SyncRoleInput {
                    name: "admin".to_string(),
                    display_name: Some("Orders Admin".to_string()),
                    description: None,
                    permissions: vec!["orders:read".to_string()],
                    client_managed: false,
                },
            ],
            remove_unlisted: false,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("orders"));
    }
}
