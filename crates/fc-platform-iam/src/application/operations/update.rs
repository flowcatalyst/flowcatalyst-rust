//! Update Application Use Case

use async_trait::async_trait;
use chrono::Utc;
use fc_platform_core::shared::id::ApplicationId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ApplicationUpdated;
use crate::application::repository::ApplicationRepository;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for updating an application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateApplicationCommand {
    /// Application ID
    pub id: ApplicationId,

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

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo_mime_type: Option<String>,
}

impl AuditMasked for UpdateApplicationCommand {}

/// Use case for updating an application.
pub struct UpdateApplicationUseCase<U: UnitOfWork> {
    application_repo: Arc<ApplicationRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateApplicationUseCase<U> {
    pub fn new(application_repo: Arc<ApplicationRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            application_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for UpdateApplicationUseCase<U> {
    type Command = UpdateApplicationCommand;
    type Event = ApplicationUpdated;

    async fn validate(&self, command: &UpdateApplicationCommand) -> Result<(), UseCaseError> {
        // Validate name if provided
        if let Some(ref name) = command.name {
            if name.trim().is_empty() {
                return Err(UseCaseError::validation(
                    "NAME_REQUIRED",
                    "name cannot be empty",
                ));
            }
        }

        Ok(())
    }

    /// Applications are platform-owner data, written by anchors only (the
    /// rule every application handler applies).
    /// The handler's gate checks this, with the permission, before the body
    /// is read; here it holds for every caller (fc-web, orchestrations).
    async fn authorize(
        &self,
        _command: &UpdateApplicationCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(checks::require_anchor_scope(ctx.caller())?)
    }

    async fn execute(
        &self,
        command: UpdateApplicationCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<ApplicationUpdated>, UseCaseError> {
        // Find the application
        let mut application = self
            .application_repo
            .find_by_id(&command.id)
            .await
            .or_not_found(
                "APPLICATION_NOT_FOUND",
                format!("Application with ID '{}' not found", command.id),
            )?;

        // Apply name update
        if let Some(ref name) = command.name {
            let name = name.trim();
            if application.name != name {
                application.name = name.to_string();
            }
        }

        // Apply description update
        if let Some(ref description) = command.description {
            application.description = Some(description.clone());
        }

        // Apply URL updates
        if let Some(ref url) = command.default_base_url {
            application.default_base_url = if url.is_empty() {
                None
            } else {
                Some(url.clone())
            };
        }

        if let Some(ref url) = command.icon_url {
            application.icon_url = if url.is_empty() {
                None
            } else {
                Some(url.clone())
            };
        }

        // Go UpdateApplication: a supplied value replaces the stored one.
        if let Some(ref website) = command.website {
            application.website = Some(website.clone());
        }
        if let Some(ref logo) = command.logo {
            application.logo = Some(logo.clone());
        }
        if let Some(ref mime) = command.logo_mime_type {
            application.logo_mime_type = Some(mime.clone());
        }

        application.updated_at = Utc::now();

        // Create domain event
        let event = ApplicationUpdated::new(&ctx, &application.id, &application.name);

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
        let cmd = UpdateApplicationCommand {
            id: ApplicationId::parse("app_123").unwrap(),
            name: Some("Updated Name".to_string()),
            description: None,
            default_base_url: Some("https://new-url.example.com".to_string()),
            icon_url: None,
            website: None,
            logo: None,
            logo_mime_type: None,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("app_123"));
    }
}
