//! Create Identity Provider Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::IdentityProviderCreated;
use crate::identity_provider::entity::IdentityProviderType;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};
use crate::IdentityProviderRepository;

/// Command for creating a new identity provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateIdentityProviderCommand {
    pub code: String,
    pub name: String,
    pub idp_type: IdentityProviderType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_issuer_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_client_secret_ref: Option<String>,
    #[serde(default)]
    pub oidc_multi_tenant: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_issuer_pattern: Option<String>,
    #[serde(default)]
    pub allowed_email_domains: Vec<String>,
}

/// Use case for creating a new identity provider.
pub struct CreateIdentityProviderUseCase<U: UnitOfWork> {
    idp_repo: Arc<IdentityProviderRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateIdentityProviderUseCase<U> {
    pub fn new(idp_repo: Arc<IdentityProviderRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            idp_repo,
            unit_of_work,
        }
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

    async fn authorize(
        &self,
        _command: &CreateIdentityProviderCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: CreateIdentityProviderCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<IdentityProviderCreated> {
        let event = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work.emit_event(event, &command).await
    }
}

impl<U: UnitOfWork> CreateIdentityProviderUseCase<U> {
    async fn prepare(
        &self,
        command: &CreateIdentityProviderCommand,
        ctx: &ExecutionContext,
    ) -> Result<IdentityProviderCreated, UseCaseError> {
        // Business rule: code must be unique
        if self.idp_repo.find_by_code(&command.code).await?.is_some() {
            return Err(UseCaseError::business_rule(
                "IDENTITY_PROVIDER_CODE_EXISTS",
                format!(
                    "Identity provider with code '{}' already exists",
                    command.code
                ),
            ));
        }

        // Parse the type
        let idp_type = command.idp_type;

        // Create entity
        let mut idp = crate::IdentityProvider::new(&command.code, &command.name, idp_type);

        // Set OIDC fields if type is OIDC
        if idp_type == IdentityProviderType::Oidc {
            idp.oidc_issuer_url = command.oidc_issuer_url.clone();
            idp.oidc_client_id = command.oidc_client_id.clone();
            idp.oidc_client_secret_ref = command.oidc_client_secret_ref.clone();
            idp.oidc_multi_tenant = command.oidc_multi_tenant;
            idp.oidc_issuer_pattern = command.oidc_issuer_pattern.clone();
        }
        idp.allowed_email_domains = command.allowed_email_domains.clone();

        // Create domain event
        let event =
            IdentityProviderCreated::new(ctx, &idp.id, &idp.code, &idp.name, idp_type.as_str());

        // Insert via repo
        if let Err(e) = self.idp_repo.insert(&idp).await {
            return Err(UseCaseError::commit(format!(
                "Failed to insert identity provider: {}",
                e
            )));
        }
        Ok(event)
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
            idp_type: IdentityProviderType::Oidc,
            oidc_issuer_url: Some("https://accounts.google.com".to_string()),
            oidc_client_id: Some("client-123".to_string()),
            oidc_client_secret_ref: None,
            oidc_multi_tenant: false,
            oidc_issuer_pattern: None,
            allowed_email_domains: vec![],
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("google-oidc"));
        assert!(json.contains("idpType"));
    }
}
