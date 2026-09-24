//! Activate Application Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ApplicationActivated;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::ApplicationRepository;

/// Command for activating an application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivateApplicationCommand {
    /// Application ID
    pub id: String,
}

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

    async fn authorize(
        &self,
        _command: &ActivateApplicationCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: ActivateApplicationCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ApplicationActivated> {
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

impl<U: UnitOfWork> ActivateApplicationUseCase<U> {
    async fn prepare(
        &self,
        command: &ActivateApplicationCommand,
        ctx: &ExecutionContext,
    ) -> Result<(crate::Application, ApplicationActivated), UseCaseError> {
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
        let event = ApplicationActivated::new(ctx, &application.id, &application.code);
        Ok((application, event))
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
