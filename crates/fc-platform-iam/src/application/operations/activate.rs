//! Activate Application Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ApplicationActivated;
use crate::application::repository::ApplicationRepository;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for activating an application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivateApplicationCommand {
    /// Application ID
    pub id: String,
}

impl AuditMasked for ActivateApplicationCommand {}

/// Use case for activating an application.
pub struct ActivateApplicationUseCase<U: UnitOfWork> {
    application_repo: Arc<ApplicationRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> ActivateApplicationUseCase<U> {
    pub fn new(application_repo: Arc<ApplicationRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            application_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for ActivateApplicationUseCase<U> {
    type Command = ActivateApplicationCommand;
    type Event = ApplicationActivated;

    async fn validate(&self, _command: &ActivateApplicationCommand) -> Result<(), UseCaseError> {
        Ok(())
    }

    /// Applications are platform-owner data, written by anchors only (the
    /// rule every application handler applies).
    /// The handler's gate checks this, with the permission, before the body
    /// is read; here it holds for every caller (fc-web, orchestrations).
    async fn authorize(
        &self,
        _command: &ActivateApplicationCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(checks::require_anchor_scope(ctx.caller())?)
    }

    async fn execute(
        &self,
        command: ActivateApplicationCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<ApplicationActivated>, UseCaseError> {
        // Find the application
        let mut application = self
            .application_repo
            .find_by_id(&command.id)
            .await
            .or_not_found(
                "APPLICATION_NOT_FOUND",
                format!("Application with ID '{}' not found", command.id),
            )?;

        // Business rule: must be inactive to activate
        if application.active {
            return Err(UseCaseError::business_rule(
                "APPLICATION_ALREADY_ACTIVE",
                "Application is already active",
            ));
        }

        // Activate the application
        application.activate();

        // Create domain event
        let event = ApplicationActivated::new(&ctx, &application.id);

        // Atomic commit
        self.unit_of_work
            .commit(&application, &*self.application_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = ActivateApplicationCommand {
            id: "app-123".to_string(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("app-123"));
    }
}
