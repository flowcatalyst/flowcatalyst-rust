//! Delete Application Use Case

use async_trait::async_trait;
use fc_platform_core::shared::id::ApplicationId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ApplicationDeleted;
use crate::application::repository::ApplicationRepository;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for deleting an application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteApplicationCommand {
    pub application_id: ApplicationId,
}

impl AuditMasked for DeleteApplicationCommand {}

pub struct DeleteApplicationUseCase<U: UnitOfWork> {
    application_repo: Arc<ApplicationRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteApplicationUseCase<U> {
    pub fn new(application_repo: Arc<ApplicationRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            application_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteApplicationUseCase<U> {
    type Command = DeleteApplicationCommand;
    type Event = ApplicationDeleted;

    async fn validate(&self, command: &DeleteApplicationCommand) -> Result<(), UseCaseError> {
        if command.application_id.as_str().trim().is_empty() {
            return Err(UseCaseError::validation(
                "APPLICATION_ID_REQUIRED",
                "Application ID is required",
            ));
        }

        Ok(())
    }

    /// Applications are platform-owner data, written by anchors only (the
    /// rule every application handler applies).
    /// The handler's gate checks this, with the permission, before the body
    /// is read; here it holds for every caller (fc-web, orchestrations).
    async fn authorize(
        &self,
        _command: &DeleteApplicationCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(checks::require_anchor_scope(ctx.caller())?)
    }

    async fn execute(
        &self,
        command: DeleteApplicationCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<ApplicationDeleted>, UseCaseError> {
        let application = self
            .application_repo
            .find_by_id(&command.application_id)
            .await
            .or_not_found(
                "APPLICATION_NOT_FOUND",
                format!("Application with ID '{}' not found", command.application_id),
            )?;

        // Business rules: refuse deletion while any code-enforced reference
        // still points at this application. None of these columns have
        // DB-level FKs — each one is a place where the app must be
        // explicitly unwired before deletion.
        let grants = self
            .application_repo
            .count_access_grants(&application.id)
            .await?;
        // Only enabled configs (owner decision #55): a disabled one is
        // deleted with the application.
        let configs = self
            .application_repo
            .count_enabled_client_configs(&application.id)
            .await?;
        let sas = self
            .application_repo
            .count_service_accounts(&application.id)
            .await?;
        let roles = self.application_repo.count_roles(&application.id).await?;
        let principal_refs = self
            .application_repo
            .count_principal_refs(&application.id)
            .await?;

        let refs = [
            ("access grants", grants),
            ("client configs", configs),
            ("service accounts", sas),
            ("application roles", roles),
            ("principal refs", principal_refs),
        ];
        let blockers: Vec<String> = refs
            .iter()
            .filter(|(_, n)| *n > 0)
            .map(|(label, n)| format!("{n} {label}"))
            .collect();
        if !blockers.is_empty() {
            return Err(UseCaseError::business_rule(
                "APPLICATION_HAS_REFERENCES",
                format!(
                    "Cannot delete application '{}' — {} still reference it. \
                     Remove those before deleting.",
                    application.code,
                    blockers.join(", "),
                ),
            ));
        }

        let event = ApplicationDeleted::new(&ctx, &application.id, &application.code);

        self.unit_of_work
            .commit_delete(&application, &*self.application_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = DeleteApplicationCommand {
            application_id: ApplicationId::parse("app_123").unwrap(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("applicationId"));
    }
}
