//! Deactivate Application Use Case

use async_trait::async_trait;
use fc_platform_core::shared::id::ApplicationId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ApplicationDeactivated;
use crate::application::repository::ApplicationRepository;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for deactivating an application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeactivateApplicationCommand {
    /// Application ID
    pub id: ApplicationId,
}

impl AuditMasked for DeactivateApplicationCommand {}

/// Use case for deactivating an application.
pub struct DeactivateApplicationUseCase<U: UnitOfWork> {
    application_repo: Arc<ApplicationRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeactivateApplicationUseCase<U> {
    pub fn new(application_repo: Arc<ApplicationRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            application_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeactivateApplicationUseCase<U> {
    type Command = DeactivateApplicationCommand;
    type Event = ApplicationDeactivated;

    async fn validate(&self, _command: &DeactivateApplicationCommand) -> Result<(), UseCaseError> {
        Ok(())
    }

    /// Applications are platform-owner data, written by anchors only (the
    /// rule every application handler applies).
    /// The handler's gate checks this, with the permission, before the body
    /// is read; here it holds for every caller (fc-web, orchestrations).
    async fn authorize(
        &self,
        _command: &DeactivateApplicationCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(checks::require_anchor_scope(ctx.caller())?)
    }

    async fn execute(
        &self,
        command: DeactivateApplicationCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<ApplicationDeactivated>, UseCaseError> {
        // Find the application
        let mut application = self
            .application_repo
            .find_by_id(&command.id)
            .await
            .or_not_found(
                "APPLICATION_NOT_FOUND",
                format!("Application with ID '{}' not found", command.id),
            )?;

        // Business rule: must be active to deactivate
        if !application.active {
            return Err(UseCaseError::business_rule(
                "APPLICATION_ALREADY_INACTIVE",
                "Application is already inactive",
            ));
        }

        // Deactivate the application
        application.deactivate();

        // Create domain event
        let event = ApplicationDeactivated::new(&ctx, &application.id);

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
        let cmd = DeactivateApplicationCommand {
            id: ApplicationId::parse("app_123").unwrap(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("app_123"));
    }
}
