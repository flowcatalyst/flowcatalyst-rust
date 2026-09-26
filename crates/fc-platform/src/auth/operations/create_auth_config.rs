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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additional_client_ids: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granted_client_ids: Option<Vec<String>>,
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

impl crate::usecase::AuditMasked for CreateAuthConfigCommand {}

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
        // Go CreateAuthConfig (auth/operations/auth_config.go): the domain
        // must hold a dot, and an OIDC provider needs its issuer and client id.
        let email_domain = command.email_domain.trim().to_lowercase();
        if email_domain.is_empty() || !email_domain.contains('.') {
            return Err(UseCaseError::validation(
                "INVALID_EMAIL_DOMAIN",
                "emailDomain must be a valid DNS name",
            ));
        }
        if command.auth_provider == Some(AuthProvider::Oidc) {
            let blank = |v: &Option<String>| v.as_deref().is_none_or(|s| s.trim().is_empty());
            if blank(&command.oidc_issuer_url) {
                return Err(UseCaseError::validation(
                    "OIDC_ISSUER_REQUIRED",
                    "OIDC provider requires oidcIssuerUrl",
                ));
            }
            if blank(&command.oidc_client_id) {
                return Err(UseCaseError::validation(
                    "OIDC_CLIENT_ID_REQUIRED",
                    "OIDC provider requires oidcClientId",
                ));
            }
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
                "DOMAIN_ALREADY_CONFIGURED",
                format!("Auth config for '{}' already exists", email_domain),
            ));
        }

        let config_type = command.config_type;
        let mut config = ClientAuthConfig::new_internal(&email_domain, config_type);

        config.primary_client_id = command.primary_client_id.clone();
        if let Some(ids) = &command.additional_client_ids {
            config.additional_client_ids = ids.clone();
        }
        if let Some(ids) = &command.granted_client_ids {
            config.granted_client_ids = ids.clone();
        }

        if let Some(provider) = command.auth_provider {
            config.auth_provider = provider;
        }
        config.oidc_issuer_url = command.oidc_issuer_url.clone();
        config.oidc_client_id = command.oidc_client_id.clone();
        config.oidc_multi_tenant = command.oidc_multi_tenant;
        config.oidc_issuer_pattern = command.oidc_issuer_pattern.clone();
        config.oidc_client_secret_ref = command.oidc_client_secret_ref.clone();

        let event = AuthConfigCreated::new(&ctx, &config.id, &config.email_domain);

        self.unit_of_work
            .commit(&config, &*self.auth_config_repo, event, &command)
            .await
    }
}
