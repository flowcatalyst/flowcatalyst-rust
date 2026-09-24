//! Create ClientAuthConfig Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::AuthConfigCreated;
use crate::auth::config_entity::{AuthConfigType, AuthProvider, ClientAuthConfig};
use crate::auth::config_repository::ClientAuthConfigRepository;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAuthConfigCommand {
    pub email_domain: String,
    pub config_type: AuthConfigType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_provider: Option<AuthProvider>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_issuer_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_client_id: Option<String>,
    #[serde(default)]
    pub oidc_multi_tenant: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_issuer_pattern: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_client_secret_ref: Option<String>,
}

pub struct CreateAuthConfigUseCase<U: UnitOfWork> {
    auth_config_repo: Arc<ClientAuthConfigRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateAuthConfigUseCase<U> {
    pub fn new(auth_config_repo: Arc<ClientAuthConfigRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            auth_config_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for CreateAuthConfigUseCase<U> {
    type Command = CreateAuthConfigCommand;
    type Event = AuthConfigCreated;

    async fn validate(&self, command: &CreateAuthConfigCommand) -> Result<(), UseCaseError> {
        let email_domain = command.email_domain.trim().to_lowercase();
        if email_domain.is_empty() {
            return Err(UseCaseError::validation(
                "EMAIL_DOMAIN_REQUIRED",
                "Email domain is required",
            ));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &CreateAuthConfigCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: CreateAuthConfigCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<AuthConfigCreated> {
        let email_domain = command.email_domain.trim().to_lowercase();

        // Business rule: email domain must be unique
        let existing = match self
            .auth_config_repo
            .find_by_email_domain(&email_domain)
            .await
        {
            Ok(found) => found,
            Err(e) => return UseCaseResult::failure(e.into()),
        };
        if existing.is_some() {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "EMAIL_DOMAIN_EXISTS",
                format!(
                    "An auth config for domain '{}' already exists",
                    email_domain
                ),
            ));
        }

        let config_type = command.config_type;
        let mut config = ClientAuthConfig::new_internal(&email_domain, config_type);

        config.primary_client_id = command.primary_client_id.clone();

        if let Some(provider) = command.auth_provider {
            config.auth_provider = provider;
        }
        config.oidc_issuer_url = command.oidc_issuer_url.clone();
        config.oidc_client_id = command.oidc_client_id.clone();
        config.oidc_multi_tenant = command.oidc_multi_tenant;
        config.oidc_issuer_pattern = command.oidc_issuer_pattern.clone();
        config.oidc_client_secret_ref = command.oidc_client_secret_ref.clone();

        let event = AuthConfigCreated::new(
            &ctx,
            &config.id,
            &config.email_domain,
            config.config_type.as_str(),
        );

        self.unit_of_work
            .commit(&config, &*self.auth_config_repo, event, &command)
            .await
    }
}
