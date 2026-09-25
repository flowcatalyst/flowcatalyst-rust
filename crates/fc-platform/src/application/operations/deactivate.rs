//! Deactivate Application Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ApplicationDeactivated;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::ApplicationRepository;

/// Command for deactivating an application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeactivateApplicationCommand {
    /// Application ID
    pub id: String,
}

impl crate::usecase::AuditMasked for DeactivateApplicationCommand {}

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

    async fn authorize(
        &self,
        _command: &DeactivateApplicationCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: DeactivateApplicationCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ApplicationDeactivated> {
        let (application, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        // Atomic commit
        self.unit_of_work
            .commit(&application, &*self.application_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> DeactivateApplicationUseCase<U> {
    async fn prepare(
        &self,
        command: &DeactivateApplicationCommand,
        ctx: &ExecutionContext,
    ) -> Result<(crate::Application, ApplicationDeactivated), UseCaseError> {
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
        let event = ApplicationDeactivated::new(ctx, &application.id);
        Ok((application, event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = DeactivateApplicationCommand {
            id: "app-123".to_string(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("app-123"));
    }
}
