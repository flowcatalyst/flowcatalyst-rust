//! Create Application Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ApplicationCreated;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};
use crate::ApplicationRepository;
use crate::{Application, ApplicationType};

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
    pub application_type: Option<ApplicationType>,

    /// Default base URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_base_url: Option<String>,

    /// Icon URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,

    /// Website URL
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,

    /// Inline SVG logo
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,

    /// Logo MIME type
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo_mime_type: Option<String>,
}

/// Go's application-code rule (`validate.CodeUnderscorePattern`,
/// `^[a-z][a-z0-9_-]*$`), applied to the lower-cased code.
fn is_valid_code(code: &str) -> bool {
    let mut chars = code.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

impl crate::usecase::AuditMasked for CreateApplicationCommand {}

/// Use case for creating a new application.
pub struct CreateApplicationUseCase<U: UnitOfWork> {
    application_repo: Arc<ApplicationRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateApplicationUseCase<U> {
    pub fn new(application_repo: Arc<ApplicationRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            application_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for CreateApplicationUseCase<U> {
    type Command = CreateApplicationCommand;
    type Event = ApplicationCreated;

    async fn validate(&self, command: &CreateApplicationCommand) -> Result<(), UseCaseError> {
        // Go CreateApplication (application/operations/create.go): the code
        // is lower-cased before it is checked and stored.
        let code = command.code.trim().to_lowercase();
        if code.is_empty() {
            return Err(UseCaseError::validation(
                "CODE_REQUIRED",
                "code is required",
            ));
        }
        if !is_valid_code(&code) {
            return Err(UseCaseError::validation(
                "INVALID_CODE_FORMAT",
                "code must start with a lowercase letter and contain only lowercase alphanumerics, hyphens, and underscores",
            ));
        }
        if command.name.trim().is_empty() {
            return Err(UseCaseError::validation(
                "NAME_REQUIRED",
                "name is required",
            ));
        }

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &CreateApplicationCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: CreateApplicationCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ApplicationCreated> {
        let code_lower = command.code.trim().to_lowercase();
        let code = code_lower.as_str();
        let name = command.name.trim();

        // Business rule: code must be unique
        let existing = match self.application_repo.find_by_code(code).await {
            Ok(found) => found,
            Err(e) => return UseCaseResult::failure(e.into()),
        };
        if existing.is_some() {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "CODE_EXISTS",
                format!("Application with code '{}' already exists", code),
            ));
        }

        // Create the application entity
        let mut application = if command.application_type == Some(ApplicationType::Integration) {
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
        application.website = command.website.clone();
        application.logo = command.logo.clone();
        application.logo_mime_type = command.logo_mime_type.clone();

        // Create domain event
        let event =
            ApplicationCreated::new(&ctx, &application.id, &application.code, &application.name);

        // Atomic commit
        self.unit_of_work
            .commit(&application, &*self.application_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::unit_of_work::HasId;

    #[test]
    fn test_command_serialization() {
        let cmd = CreateApplicationCommand {
            code: "orders".to_string(),
            name: "Orders Application".to_string(),
            description: Some("Handles order processing".to_string()),
            application_type: Some(ApplicationType::Application),
            default_base_url: Some("https://orders.example.com".to_string()),
            icon_url: None,
            website: None,
            logo: None,
            logo_mime_type: None,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("orders"));
    }

    #[test]
    fn test_application_has_id() {
        let app = Application::new("test", "Test");
        assert!(!app.id().is_empty());
    }
}
