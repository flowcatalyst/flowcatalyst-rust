//! Create Identity Provider Use Case

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::IdentityProviderRepository;
use crate::identity_provider::entity::IdentityProviderType;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};
use super::events::IdentityProviderCreated;

/// Command for creating a new identity provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateIdentityProviderCommand {
    pub code: String,
    pub name: String,
    pub idp_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_issuer_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_client_secret_ref: Option<String>,
}

/// Use case for creating a new identity provider.
pub struct CreateIdentityProviderUseCase<U: UnitOfWork> {
    idp_repo: Arc<IdentityProviderRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateIdentityProviderUseCase<U> {
    pub fn new(idp_repo: Arc<IdentityProviderRepository>, unit_of_work: Arc<U>) -> Self {
        Self { idp_repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for CreateIdentityProviderUseCase<U> {
    type Command = CreateIdentityProviderCommand;
    type Event = IdentityProviderCreated;

    async fn validate(&self, command: &CreateIdentityProviderCommand) -> Result<(), UseCaseError> {
        if command.code.trim().is_empty() {
            return Err(UseCaseError::validation(
                "CODE_REQUIRED",
                "Identity provider code is required",
            ));
        }

        if command.name.trim().is_empty() {
            return Err(UseCaseError::validation(
                "NAME_REQUIRED",
                "Identity provider name is required",
            ));
        }

        Ok(())
    }

    async fn authorize(&self, _command: &CreateIdentityProviderCommand, _ctx: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: CreateIdentityProviderCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<IdentityProviderCreated> {
        // Business rule: code must be unique
        match self.idp_repo.find_by_code(&command.code).await {
            Ok(Some(_)) => {
                return UseCaseResult::failure(UseCaseError::business_rule(
                    "IDENTITY_PROVIDER_CODE_EXISTS",
                    format!("Identity provider with code '{}' already exists", command.code),
                ));
            }
            Ok(None) => {}
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to check identity provider code: {}", e
                )));
            }
        }

        // Parse the type
        let idp_type = IdentityProviderType::from_str(&command.idp_type);

        // Create entity
        let mut idp = crate::IdentityProvider::new(&command.code, &command.name, idp_type);

        // Set OIDC fields if type is OIDC
        if idp_type == IdentityProviderType::Oidc {
            idp.oidc_issuer_url = command.oidc_issuer_url.clone();
            idp.oidc_client_id = command.oidc_client_id.clone();
            idp.oidc_client_secret_ref = command.oidc_client_secret_ref.clone();
        }

        // Create domain event
        let event = IdentityProviderCreated::new(
            &ctx,
            &idp.id,
            &idp.code,
            &idp.name,
            idp_type.as_str(),
        );

        // Insert via repo
        if let Err(e) = self.idp_repo.insert(&idp).await {
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to insert identity provider: {}", e
            )));
        }

        self.unit_of_work.emit_event(event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = CreateIdentityProviderCommand {
            code: "google-oidc".to_string(),
            name: "Google OIDC".to_string(),
            idp_type: "OIDC".to_string(),
            oidc_issuer_url: Some("https://accounts.google.com".to_string()),
            oidc_client_id: Some("client-123".to_string()),
            oidc_client_secret_ref: None,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("google-oidc"));
        assert!(json.contains("idpType"));
    }
}
