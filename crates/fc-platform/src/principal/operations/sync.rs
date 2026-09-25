//! Sync Principals Use Case
//!
//! Syncs user principals from an application SDK. For each principal:
//! - Find by email: if exists, update name and sync roles
//! - If not found: create new user and assign roles
//!
//! If removeUnlisted is true, SDK_SYNC roles are removed from principals
//! not in the sync list (principals are not deleted).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::PrincipalsSynced;
use crate::principal::entity::{Principal, UserScope};
use crate::service_account::entity::{AssignmentSource, RoleAssignment};
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::ApplicationRepository;
use crate::PrincipalRepository;

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
    /// A password hash (e.g. Laravel's bcrypt `$2y$`) stored verbatim on a
    /// user this sync creates; login verifies it and re-encodes it. Never
    /// applied to an existing user (decision #22). Masked in the audit row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_hash: Option<String>,
}

fn default_active() -> bool {
    true
}

/// Command for syncing principals from an application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPrincipalsCommand {
    pub application_code: String,
    pub principals: Vec<SyncPrincipalInput>,
    #[serde(default)]
    pub remove_unlisted: bool,
}

impl crate::usecase::AuditMasked for SyncPrincipalsCommand {}

pub struct SyncPrincipalsUseCase<U: UnitOfWork> {
    principal_repo: Arc<PrincipalRepository>,
    application_repo: Arc<ApplicationRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> SyncPrincipalsUseCase<U> {
    pub fn new(
        principal_repo: Arc<PrincipalRepository>,
        application_repo: Arc<ApplicationRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            principal_repo,
            application_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for SyncPrincipalsUseCase<U> {
    type Command = SyncPrincipalsCommand;
    type Event = PrincipalsSynced;

    async fn validate(&self, command: &SyncPrincipalsCommand) -> Result<(), UseCaseError> {
        if command.application_code.trim().is_empty() {
            return Err(UseCaseError::validation(
                "APPLICATION_CODE_REQUIRED",
                "Application code is required",
            ));
        }

        if command.principals.is_empty() {
            return Err(UseCaseError::validation(
                "PRINCIPALS_REQUIRED",
                "At least one principal must be provided",
            ));
        }

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &SyncPrincipalsCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: SyncPrincipalsCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<PrincipalsSynced> {
        let event = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work.emit_event(event, &command).await
    }
}

impl<U: UnitOfWork> SyncPrincipalsUseCase<U> {
    async fn prepare(
        &self,
        command: &SyncPrincipalsCommand,
        ctx: &ExecutionContext,
    ) -> Result<PrincipalsSynced, UseCaseError> {
        // Verify the application exists
        self.application_repo
            .find_by_code(&command.application_code)
            .await
            .or_not_found(
                "APPLICATION_NOT_FOUND",
                format!("Application not found: {}", command.application_code),
            )?;

        let mut created_count = 0u32;
        let mut updated_count = 0u32;
        let mut deactivated_count = 0u32;
        let mut synced_emails: Vec<String> = Vec::new();

        for input in &command.principals {
            let email = input.email.to_lowercase();
            synced_emails.push(email.clone());

            // Build SDK_SYNC role assignments
            let role_assignments: Vec<RoleAssignment> = input
                .roles
                .iter()
                .map(|r| RoleAssignment::with_source(r.to_lowercase(), AssignmentSource::SdkSync))
                .collect();

            match self.principal_repo.find_by_email(&email).await? {
                Some(mut principal) => {
                    // Merge: keep non-SDK_SYNC roles, replace SDK_SYNC roles
                    let non_sdk_roles: Vec<RoleAssignment> = principal
                        .roles
                        .iter()
                        .filter(|r| !r.has_source(AssignmentSource::SdkSync))
                        .cloned()
                        .collect();
                    let mut merged = non_sdk_roles;
                    merged.extend(role_assignments);
                    principal.roles = merged;
                    principal.name = input.name.clone();
                    principal.active = input.active;
                    principal.updated_at = chrono::Utc::now();
                    if input
                        .password_hash
                        .as_deref()
                        .is_some_and(|h| !h.is_empty())
                    {
                        // Decision #22: a hash is used only to create.
                        tracing::info!(
                            principal_id = %principal.id,
                            "principal sync: passwordHash ignored for an existing principal"
                        );
                    }

                    if let Err(e) = self.principal_repo.update(&principal).await {
                        return Err(UseCaseError::commit(format!(
                            "Failed to update principal '{}': {}",
                            email, e
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
                    // Go sdksync/api.go:527: carry a migrated credential
                    // verbatim so the user keeps their password.
                    if let (Some(hash), Some(identity)) = (
                        input.password_hash.as_deref().filter(|h| !h.is_empty()),
                        principal.user_identity.as_mut(),
                    ) {
                        identity.password_hash = Some(hash.to_string());
                    }

                    if let Err(e) = self.principal_repo.insert(&principal).await {
                        return Err(UseCaseError::commit(format!(
                            "Failed to create principal '{}': {}",
                            email, e
                        )));
                    }
                    created_count += 1;
                }
            }
        }

        // Remove SDK_SYNC roles from unlisted principals
        if command.remove_unlisted {
            let all_principals = self.principal_repo.find_all().await?;

            for principal in all_principals {
                if !principal.is_user() {
                    continue;
                }
                let email = match principal.email() {
                    Some(e) => e.to_string(),
                    None => continue,
                };
                if synced_emails.contains(&email) {
                    continue;
                }

                let has_sdk_roles = principal
                    .roles
                    .iter()
                    .any(|r| r.has_source(AssignmentSource::SdkSync));

                if has_sdk_roles {
                    let mut updated = principal.clone();
                    updated
                        .roles
                        .retain(|r| !r.has_source(AssignmentSource::SdkSync));
                    updated.updated_at = chrono::Utc::now();
                    if let Err(e) = self.principal_repo.update(&updated).await {
                        return Err(UseCaseError::commit(format!(
                            "Failed to update principal '{}': {}",
                            email, e
                        )));
                    }
                    deactivated_count += 1;
                }
            }
        }

        let event = PrincipalsSynced {
            metadata: PrincipalsSynced::metadata_for(ctx, &command.application_code),
            application_code: command.application_code.clone(),
            created: created_count,
            updated: updated_count,
            deactivated: deactivated_count,
            synced_emails,
        };
        Ok(event)
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
