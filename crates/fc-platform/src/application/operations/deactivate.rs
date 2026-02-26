//! Deactivate Application Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::ApplicationRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::ApplicationDeactivated;

/// Command for deactivating an application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeactivateApplicationCommand {
    /// Application ID
    pub id: String,
}

/// Use case for deactivating an application.
pub struct DeactivateApplicationUseCase<U: UnitOfWork> {
    application_repo: Arc<ApplicationRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeactivateApplicationUseCase<U> {
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
        command: DeactivateApplicationCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ApplicationDeactivated> {
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

        // Business rule: must be active to deactivate
        if !application.active {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "APPLICATION_ALREADY_INACTIVE",
                "Application is already inactive",
            ));
        }

        // Deactivate the application
        application.deactivate();

        // Create domain event
        let event = ApplicationDeactivated::new(
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
        let cmd = DeactivateApplicationCommand {
            id: "app-123".to_string(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("app-123"));
    }
}
