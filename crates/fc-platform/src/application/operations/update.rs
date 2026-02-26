//! Update Application Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};
use chrono::Utc;

use crate::ApplicationRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::ApplicationUpdated;

/// Command for updating an application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateApplicationCommand {
    /// Application ID
    pub id: String,

    /// Updated name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Updated description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Updated default base URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_base_url: Option<String>,

    /// Updated icon URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
}

/// Use case for updating an application.
pub struct UpdateApplicationUseCase<U: UnitOfWork> {
    application_repo: Arc<ApplicationRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateApplicationUseCase<U> {
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
        command: UpdateApplicationCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ApplicationUpdated> {
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

        // Track changes for event
        let mut updated_name: Option<String> = None;
        let mut updated_description: Option<String> = None;

        // Apply name update
        if let Some(ref name) = command.name {
            let name = name.trim();
            if name.is_empty() {
                return UseCaseResult::failure(UseCaseError::validation(
                    "INVALID_NAME",
                    "Name cannot be empty",
                ));
            }
            if application.name != name {
                application.name = name.to_string();
                updated_name = Some(name.to_string());
            }
        }

        // Apply description update
        if let Some(ref description) = command.description {
            application.description = Some(description.clone());
            updated_description = Some(description.clone());
        }

        // Apply URL updates
        if let Some(ref url) = command.default_base_url {
            application.default_base_url = if url.is_empty() { None } else { Some(url.clone()) };
        }

        if let Some(ref url) = command.icon_url {
            application.icon_url = if url.is_empty() { None } else { Some(url.clone()) };
        }

        application.updated_at = Utc::now();

        // Create domain event
        let event = ApplicationUpdated::new(
            &ctx,
            &application.id,
            updated_name.as_deref(),
            updated_description.as_deref(),
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
        let cmd = UpdateApplicationCommand {
            id: "app-123".to_string(),
            name: Some("Updated Name".to_string()),
            description: None,
            default_base_url: Some("https://new-url.example.com".to_string()),
            icon_url: None,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("app-123"));
    }
}
