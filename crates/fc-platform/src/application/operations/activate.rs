//! Activate Application Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::ApplicationRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::ApplicationActivated;

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
    pub fn new(
        application_repo: Arc<ApplicationRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            application_repo,
            unit_of_work,
        }
    }

    pub async fn execute(
        &self,
        command: ActivateApplicationCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ApplicationActivated> {
        // Find the application
        let mut application = match self.application_repo.find_by_id(&command.id).await {
            Ok(Some(app)) => app,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "APPLICATION_NOT_FOUND",
                    format!("Application with ID '{}' not found", command.id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(
                    format!("Failed to find application: {}", e),
                ));
            }
        };

        // Business rule: must be inactive to activate
        if application.active {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "APPLICATION_ALREADY_ACTIVE",
                "Application is already active",
            ));
        }

        // Activate the application
        application.activate();

        // Create domain event
        let event = ApplicationActivated::new(
            &ctx,
            &application.id,
            &application.code,
        );

        // Atomic commit
        self.unit_of_work.commit(&application, event, &command).await
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
