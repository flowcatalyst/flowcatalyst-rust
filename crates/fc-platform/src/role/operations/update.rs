//! Update Role Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

use super::events::RoleUpdated;
use crate::role::entity::{AuthRole, RoleSource};
use crate::role::repository::RoleRepository;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

/// Command for updating an existing role.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRoleCommand {
    /// Role ID to update
    pub role_id: String,

    /// New display name (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// New description (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// New permissions (replaces existing if provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Vec<String>>,

    /// Whether clients can manage this role
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_managed: Option<bool>,

    /// May the permissions added include another application's? Only the
    /// admin API sets this, for a super-admin (owner ruling 15); recorded in
    /// the audit row when set.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cross_application: bool,
}

impl crate::usecase::AuditMasked for UpdateRoleCommand {}

/// Use case for updating an existing role.
pub struct UpdateRoleUseCase<U: UnitOfWork> {
    role_repo: Arc<RoleRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateRoleUseCase<U> {
    pub fn new(role_repo: Arc<RoleRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            role_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for UpdateRoleUseCase<U> {
    type Command = UpdateRoleCommand;
    type Event = RoleUpdated;

    async fn validate(&self, command: &UpdateRoleCommand) -> Result<(), UseCaseError> {
        if command.role_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "ROLE_ID_REQUIRED",
                "Role ID is required",
            ));
        }

        if command.display_name.is_none()
            && command.description.is_none()
            && command.permissions.is_none()
            && command.client_managed.is_none()
        {
            return Err(UseCaseError::validation(
                "NO_UPDATES",
                "At least one field must be provided for update",
            ));
        }

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &UpdateRoleCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: UpdateRoleCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<RoleUpdated> {
        let (role, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        // Atomic commit
        self.unit_of_work
            .commit(&role, &*self.role_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> UpdateRoleUseCase<U> {
    async fn prepare(
        &self,
        command: &UpdateRoleCommand,
        ctx: &ExecutionContext,
    ) -> Result<(AuthRole, RoleUpdated), UseCaseError> {
        // Fetch existing role
        let mut role = self
            .role_repo
            .find_by_id(&command.role_id)
            .await
            .or_not_found(
                "ROLE_NOT_FOUND",
                format!("Role with ID '{}' not found", command.role_id),
            )?;

        // Business rule: can only update database-defined roles
        if role.source != RoleSource::Database {
            return Err(UseCaseError::business_rule(
                "CANNOT_MODIFY_ROLE",
                "Cannot modify a code-defined or SDK-synced role",
            ));
        }

        // Track changes
        let mut updated_display_name: Option<&str> = None;
        let mut updated_description: Option<&str> = None;
        let mut permissions_added: Vec<String> = Vec::new();
        let mut permissions_removed: Vec<String> = Vec::new();
        let mut client_managed_changed = false;

        // Apply updates
        if let Some(ref name) = command.display_name {
            let name = name.trim();
            if name != role.display_name {
                role.display_name = name.to_string();
                updated_display_name = Some(name);
            }
        }

        if let Some(ref desc) = command.description {
            let changed = role.description.as_deref() != Some(desc.as_str());
            if changed {
                role.description = Some(desc.clone());
                updated_description = Some(desc.as_str());
            }
        }

        if let Some(cm) = command.client_managed {
            if cm != role.client_managed {
                role.client_managed = cm;
                client_managed_changed = true;
            }
        }

        if let Some(ref new_permissions) = command.permissions {
            let new_set: HashSet<String> = new_permissions.iter().cloned().collect();
            let old_set = role.permissions.clone();

            // Calculate diff
            permissions_added = new_set.difference(&old_set).cloned().collect();
            permissions_removed = old_set.difference(&new_set).cloned().collect();

            // Owner ruling 15, on what this write adds.
            super::require_confined(
                role.owning_application_code(),
                permissions_added.iter().map(String::as_str),
                command.cross_application,
            )?;

            if !permissions_added.is_empty() || !permissions_removed.is_empty() {
                role.permissions = new_set;
            }
        }

        // Check if anything actually changed
        if updated_display_name.is_none()
            && updated_description.is_none()
            && !client_managed_changed
            && permissions_added.is_empty()
            && permissions_removed.is_empty()
        {
            return Err(UseCaseError::validation(
                "NO_CHANGES",
                "No changes detected",
            ));
        }

        role.updated_at = chrono::Utc::now();

        // Create domain event
        let event = RoleUpdated::new(ctx, &role.id, &role.name);
        Ok((role, event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = UpdateRoleCommand {
            role_id: "role-123".to_string(),
            display_name: Some("New Name".to_string()),
            description: None,
            permissions: Some(vec!["orders:read".to_string()]),
            client_managed: None,
            cross_application: false,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("roleId"));
        assert!(json.contains("New Name"));
    }
}
