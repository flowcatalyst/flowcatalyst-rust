//! Assign Application Access Use Case
//!
//! Sets which applications a user or service account can access.
//! Computes delta (added/removed) and persists via UnitOfWork.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ApplicationAccessAssigned;
use crate::application::repository::ApplicationRepository;
use crate::principal::repository::PrincipalRepository;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};
use std::collections::HashSet;

/// Command for assigning application access to a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignApplicationAccessCommand {
    pub user_id: String,
    pub application_ids: Vec<String>,
    /// Sets the all-applications flag; `None` leaves it unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all_applications: Option<bool>,
}

impl AuditMasked for AssignApplicationAccessCommand {}

pub struct AssignApplicationAccessUseCase<U: UnitOfWork> {
    principal_repo: Arc<PrincipalRepository>,
    application_repo: Arc<ApplicationRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> AssignApplicationAccessUseCase<U> {
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
impl<U: UnitOfWork> UseCase for AssignApplicationAccessUseCase<U> {
    type Command = AssignApplicationAccessCommand;
    type Event = ApplicationAccessAssigned;

    async fn validate(&self, command: &AssignApplicationAccessCommand) -> Result<(), UseCaseError> {
        if command.user_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "USER_ID_REQUIRED",
                "User ID is required",
            ));
        }

        Ok(())
    }

    /// The target must be a user the caller administers (Go `requireUserAdmin`,
    /// post-load; out of reach is `User_NOT_FOUND`). Granting every application
    /// needs a caller that reaches every application itself (Go's rule; the
    /// `/api` handler checks it first, where Go does, and attaches the scope).
    async fn authorize(
        &self,
        command: &AssignApplicationAccessCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        super::access::load_administered_user(
            &self.principal_repo,
            ctx.caller(),
            &command.user_id,
            "User",
        )
        .await?;
        if command.all_applications == Some(true) {
            checks::require_all_applications_grantor(ctx.caller().application_scope())?;
        }
        Ok(())
    }

    async fn execute(
        &self,
        command: AssignApplicationAccessCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<ApplicationAccessAssigned>, UseCaseError> {
        // Find the principal
        let mut principal = self
            .principal_repo
            .find_by_id(&command.user_id)
            .await
            .or_not_found(
                "USER_NOT_FOUND",
                format!("User not found: {}", command.user_id),
            )?;

        // Users and service accounts both carry application access (Go
        // assigns to both); a new service account has none until it is
        // granted here.

        // Validate all requested applications exist, in one query.
        let apps = self
            .application_repo
            .find_by_ids(&command.application_ids)
            .await?;
        for app_id in &command.application_ids {
            match apps.get(app_id) {
                Some(app) => {
                    if !app.active {
                        return Err(UseCaseError::business_rule(
                            "APPLICATION_INACTIVE",
                            format!("Application is not active: {}", app_id),
                        ));
                    }
                }
                None => {
                    return Err(UseCaseError::validation(
                        "APPLICATION_NOT_FOUND",
                        format!("Application not found: {}", app_id),
                    ));
                }
            }
        }

        // Compute delta
        let current: HashSet<&str> = principal
            .accessible_application_ids
            .iter()
            .map(|s| s.as_str())
            .collect();
        let requested: HashSet<&str> = command.application_ids.iter().map(|s| s.as_str()).collect();

        let added: Vec<String> = requested
            .difference(&current)
            .map(|s| s.to_string())
            .collect();
        let removed: Vec<String> = current
            .difference(&requested)
            .map(|s| s.to_string())
            .collect();

        // Update principal
        principal.accessible_application_ids = command.application_ids.clone();
        if let Some(all) = command.all_applications {
            principal.all_applications = all;
        }
        principal.updated_at = chrono::Utc::now();

        let event = ApplicationAccessAssigned::new(
            &ctx,
            &principal.id,
            command.application_ids.clone(),
            added,
            removed,
        );

        self.unit_of_work
            .commit(&principal, &*self.principal_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = AssignApplicationAccessCommand {
            user_id: "user-123".to_string(),
            application_ids: vec!["app-1".to_string(), "app-2".to_string()],
            all_applications: Some(false),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("userId"));
        assert!(json.contains("applicationIds"));
        assert!(json.contains(r#""allApplications":false"#));
    }
}
