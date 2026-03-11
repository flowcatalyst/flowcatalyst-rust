//! Sync Principals Use Case
//!
//! Syncs user principals from an application SDK. For each principal:
//! - Find by email: if exists, update name and sync roles
//! - If not found: create new user and assign roles
//!
//! If removeUnlisted is true, SDK_SYNC roles are removed from principals
//! not in the sync list (principals are not deleted).

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::principal::entity::{Principal, UserScope};
use crate::service_account::entity::RoleAssignment;
use crate::PrincipalRepository;
use crate::ApplicationRepository;
use crate::usecase::{
    ExecutionContext, UseCaseError, UseCaseResult,
};
use super::events::PrincipalsSynced;

/// A single principal definition in the sync payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPrincipalInput {
    /// User's email address (unique identifier for matching)
    pub email: String,
    /// Display name
    pub name: String,
    /// Role short names to assign (prefixed with applicationCode)
    #[serde(default)]
    pub roles: Vec<String>,
    /// Whether the user is active (default: true)
    #[serde(default = "default_active")]
    pub active: bool,
}

fn default_active() -> bool { true }

/// Command for syncing principals from an application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPrincipalsCommand {
    pub application_code: String,
    pub principals: Vec<SyncPrincipalInput>,
    #[serde(default)]
    pub remove_unlisted: bool,
}

pub struct SyncPrincipalsUseCase {
    principal_repo: Arc<PrincipalRepository>,
    application_repo: Arc<ApplicationRepository>,
}

impl SyncPrincipalsUseCase {
    pub fn new(
        principal_repo: Arc<PrincipalRepository>,
        application_repo: Arc<ApplicationRepository>,
    ) -> Self {
        Self { principal_repo, application_repo }
    }

    pub async fn execute(
        &self,
        command: SyncPrincipalsCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<PrincipalsSynced> {
        if command.application_code.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "APPLICATION_CODE_REQUIRED", "Application code is required",
            ));
        }

        if command.principals.is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "PRINCIPALS_REQUIRED", "At least one principal must be provided",
            ));
        }

        // Verify the application exists
        match self.application_repo.find_by_code(&command.application_code).await {
            Ok(Some(_)) => {}
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
        }

        let mut created_count = 0u32;
        let mut updated_count = 0u32;
        let mut deactivated_count = 0u32;
        let mut synced_emails: Vec<String> = Vec::new();

        for input in &command.principals {
            let email = input.email.to_lowercase();
            synced_emails.push(email.clone());

            // Build SDK_SYNC role assignments
            let role_assignments: Vec<RoleAssignment> = input.roles.iter()
                .map(|r| RoleAssignment::with_source(r.to_lowercase(), "SDK_SYNC"))
                .collect();

            let existing = match self.principal_repo.find_by_email(&email).await {
                Ok(Some(p)) => Some(p),
                Ok(None) => None,
                Err(e) => {
                    return UseCaseResult::failure(UseCaseError::commit(format!(
                        "Failed to look up principal by email: {}", e
                    )));
                }
            };

            match existing {
                Some(mut principal) => {
                    // Merge: keep non-SDK_SYNC roles, replace SDK_SYNC roles
                    let non_sdk_roles: Vec<RoleAssignment> = principal.roles.iter()
                        .filter(|r| r.assignment_source.as_deref() != Some("SDK_SYNC"))
                        .cloned()
                        .collect();
                    let mut merged = non_sdk_roles;
                    merged.extend(role_assignments);
                    principal.roles = merged;
                    principal.name = input.name.clone();
                    principal.active = input.active;
                    principal.updated_at = chrono::Utc::now();

                    if let Err(e) = self.principal_repo.update(&principal).await {
                        return UseCaseResult::failure(UseCaseError::commit(format!(
                            "Failed to update principal '{}': {}", email, e
                        )));
                    }
                    updated_count += 1;
                }
                None => {
                    // Create new user principal
                    let mut principal = Principal::new_user(&email, UserScope::Client);
                    principal.name = input.name.clone();
                    principal.active = input.active;
                    principal.roles = role_assignments;

                    if let Err(e) = self.principal_repo.insert(&principal).await {
                        return UseCaseResult::failure(UseCaseError::commit(format!(
                            "Failed to create principal '{}': {}", email, e
                        )));
                    }
                    created_count += 1;
                }
            }
        }

        // Remove SDK_SYNC roles from unlisted principals
        if command.remove_unlisted {
            let all_principals = match self.principal_repo.find_all().await {
                Ok(list) => list,
                Err(e) => {
                    return UseCaseResult::failure(UseCaseError::commit(format!(
                        "Failed to fetch all principals: {}", e
                    )));
                }
            };

            for principal in all_principals {
                if !principal.is_user() { continue; }
                let email = match principal.email() {
                    Some(e) => e.to_string(),
                    None => continue,
                };
                if synced_emails.contains(&email) { continue; }

                let has_sdk_roles = principal.roles.iter()
                    .any(|r| r.assignment_source.as_deref() == Some("SDK_SYNC"));

                if has_sdk_roles {
                    let mut updated = principal.clone();
                    updated.roles.retain(|r| r.assignment_source.as_deref() != Some("SDK_SYNC"));
                    updated.updated_at = chrono::Utc::now();
                    if let Err(e) = self.principal_repo.update(&updated).await {
                        return UseCaseResult::failure(UseCaseError::commit(format!(
                            "Failed to update principal '{}': {}", email, e
                        )));
                    }
                    deactivated_count += 1;
                }
            }
        }

        let event = PrincipalsSynced::new(
            &ctx,
            &command.application_code,
            created_count,
            updated_count,
            deactivated_count,
            synced_emails,
        );

        UseCaseResult::success(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = SyncPrincipalsCommand {
            application_code: "orders".to_string(),
            principals: vec![],
            remove_unlisted: false,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("orders"));
    }
}
