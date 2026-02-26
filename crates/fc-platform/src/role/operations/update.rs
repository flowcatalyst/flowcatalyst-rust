//! Update Role Use Case

use std::sync::Arc;
use std::collections::HashSet;
use serde::{Deserialize, Serialize};

use crate::role::entity::RoleSource;
use crate::role::repository::RoleRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::RoleUpdated;

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
}

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

    pub async fn execute(
        &self,
        command: UpdateRoleCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<RoleUpdated> {
        // Validation: role_id is required
        if command.role_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "ROLE_ID_REQUIRED",
                "Role ID is required",
            ));
        }

        // Validation: at least one field to update
        if command.display_name.is_none()
            && command.description.is_none()
            && command.permissions.is_none()
        {
            return UseCaseResult::failure(UseCaseError::validation(
                "NO_UPDATES",
                "At least one field must be provided for update",
            ));
        }

        // Fetch existing role
        let mut role = match self.role_repo.find_by_id(&command.role_id).await {
            Ok(Some(r)) => r,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "ROLE_NOT_FOUND",
                    format!("Role with ID '{}' not found", command.role_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch role: {}",
                    e
                )));
            }
        };

        // Business rule: can only update database-defined roles
        if role.source != RoleSource::Database {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "CANNOT_MODIFY_ROLE",
                "Cannot modify a code-defined or SDK-synced role",
            ));
        }

        // Track changes
        let mut updated_display_name: Option<&str> = None;
        let mut updated_description: Option<&str> = None;
        let mut permissions_added: Vec<String> = Vec::new();
        let mut permissions_removed: Vec<String> = Vec::new();

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

        if let Some(ref new_permissions) = command.permissions {
            let new_set: HashSet<String> = new_permissions.iter().cloned().collect();
            let old_set = role.permissions.clone();

            // Calculate diff
            permissions_added = new_set.difference(&old_set).cloned().collect();
            permissions_removed = old_set.difference(&new_set).cloned().collect();

            if !permissions_added.is_empty() || !permissions_removed.is_empty() {
                role.permissions = new_set;
            }
        }

        // Check if anything actually changed
        if updated_display_name.is_none()
            && updated_description.is_none()
            && permissions_added.is_empty()
            && permissions_removed.is_empty()
        {
            return UseCaseResult::failure(UseCaseError::validation(
                "NO_CHANGES",
                "No changes detected",
            ));
        }

        role.updated_at = chrono::Utc::now();

        // Create domain event
        let event = RoleUpdated::new(
            &ctx,
            &role.id,
            updated_display_name,
            updated_description,
            permissions_added,
            permissions_removed,
        );

        // Atomic commit
        self.unit_of_work.commit(&role, event, &command).await
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
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("roleId"));
        assert!(json.contains("New Name"));
    }
}
