//! Create Application Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::{Application, ApplicationType};
use crate::ApplicationRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
    unit_of_work::HasId,
};
use super::events::ApplicationCreated;

impl HasId for Application {
    fn id(&self) -> &str {
        &self.id
    }

    fn collection_name() -> &'static str {
        "applications"
    }
}

/// Command for creating a new application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateApplicationCommand {
    /// Unique code (URL-safe)
    pub code: String,

    /// Human-readable name
    pub name: String,

    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Application type: APPLICATION or INTEGRATION
    #[serde(rename = "type")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_type: Option<String>,

    /// Default base URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_base_url: Option<String>,

    /// Icon URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
}

/// Use case for creating a new application.
pub struct CreateApplicationUseCase<U: UnitOfWork> {
    application_repo: Arc<ApplicationRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateApplicationUseCase<U> {
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
        command: CreateApplicationCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ApplicationCreated> {
        // Validation: code is required
        let code = command.code.trim();
        if code.is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "CODE_REQUIRED",
                "Application code is required",
            ));
        }

        // Validation: name is required
        let name = command.name.trim();
        if name.is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "NAME_REQUIRED",
                "Application name is required",
            ));
        }

        // Business rule: code must be unique
        let existing = self.application_repo.find_by_code(code).await;
        if let Ok(Some(_)) = existing {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "APPLICATION_CODE_EXISTS",
                format!("An application with code '{}' already exists", code),
            ));
        }

        // Create the application entity
        let mut application = if command.application_type.as_deref() == Some("INTEGRATION") {
            Application::integration(code, name)
        } else {
            Application::new(code, name)
        };

        if let Some(ref desc) = command.description {
            application = application.with_description(desc);
        }

        if let Some(ref url) = command.default_base_url {
            application = application.with_base_url(url);
        }

        if let Some(ref url) = command.icon_url {
            application = application.with_icon_url(url);
        }

        application.created_by = Some(ctx.principal_id.clone());

        // Create domain event
        let app_type = match application.application_type {
            ApplicationType::Integration => "INTEGRATION",
            ApplicationType::Application => "APPLICATION",
        };

        let event = ApplicationCreated::new(
            &ctx,
            &application.id,
            &application.code,
            &application.name,
            app_type,
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
        let cmd = CreateApplicationCommand {
            code: "orders".to_string(),
            name: "Orders Application".to_string(),
            description: Some("Handles order processing".to_string()),
            application_type: Some("APPLICATION".to_string()),
            default_base_url: Some("https://orders.example.com".to_string()),
            icon_url: None,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("orders"));
    }

    #[test]
    fn test_application_has_id() {
        let app = Application::new("test", "Test");
        assert!(!app.id().is_empty());
        assert_eq!(Application::collection_name(), "applications");
    }
}
