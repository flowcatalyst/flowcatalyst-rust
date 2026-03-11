//! Create Role Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::role::entity::AuthRole;
use crate::role::repository::RoleRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::RoleCreated;

/// Command for creating a new role.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRoleCommand {
    /// Application code (e.g., "orders", "platform")
    pub application_code: String,

    /// Role name (will be combined with app code to form code)
    pub role_name: String,

    /// Human-readable display name
    pub display_name: String,

    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Initial permissions to grant
    #[serde(default)]
    pub permissions: Vec<String>,

    /// Whether clients can manage this role
    #[serde(default)]
    pub client_managed: bool,
}


/// Use case for creating a new role.
pub struct CreateRoleUseCase<U: UnitOfWork> {
    role_repo: Arc<RoleRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateRoleUseCase<U> {
    pub fn new(role_repo: Arc<RoleRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            role_repo,
            unit_of_work,
        }
    }

    pub async fn execute(
        &self,
        command: CreateRoleCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<RoleCreated> {
        // Validation: application_code is required
        let app_code = command.application_code.trim().to_lowercase();
        if app_code.is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "APPLICATION_CODE_REQUIRED",
                "Application code is required",
            ));
        }

        // Validation: role_name is required
        let role_name = command.role_name.trim().to_lowercase();
        if role_name.is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "ROLE_NAME_REQUIRED",
                "Role name is required",
            ));
        }

        // Validation: display_name is required
        let display_name = command.display_name.trim();
        if display_name.is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "DISPLAY_NAME_REQUIRED",
                "Display name is required",
            ));
        }

        // Build role code
        let code = format!("{}:{}", app_code, role_name);

        // Business rule: name must be unique
        if let Ok(Some(_)) = self.role_repo.find_by_name(&code).await {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "ROLE_CODE_EXISTS",
                format!("A role with code '{}' already exists", code),
            ));
        }

        // Create the role entity
        let mut role = AuthRole::new(&app_code, &role_name, display_name);

        if let Some(desc) = &command.description {
            role.description = Some(desc.clone());
        }

        for perm in &command.permissions {
            role.permissions.insert(perm.clone());
        }

        role.client_managed = command.client_managed;
        // Create domain event
        let permissions_vec: Vec<String> = role.permissions.iter().cloned().collect();
        let event = RoleCreated::new(
            &ctx,
            &role.id,
            &role.name,
            &role.display_name,
            &role.application_code,
            permissions_vec,
        );

        // Atomic commit
        self.unit_of_work.commit(&role, event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::unit_of_work::HasId;

    #[test]
    fn test_command_serialization() {
        let cmd = CreateRoleCommand {
            application_code: "orders".to_string(),
            role_name: "admin".to_string(),
            display_name: "Orders Admin".to_string(),
            description: Some("Full access to orders".to_string()),
            permissions: vec!["orders:read".to_string(), "orders:write".to_string()],
            client_managed: false,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("Orders Admin"));
        assert!(json.contains("orders:read"));
    }

    #[test]
    fn test_role_has_id() {
        let role = AuthRole::new("orders", "admin", "Orders Admin");
        assert!(!role.id().is_empty());
    }
}
