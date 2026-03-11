//! Create ClientAuthConfig Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::auth::config_entity::{ClientAuthConfig, AuthConfigType, AuthProvider};
use crate::auth::config_repository::ClientAuthConfigRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::AuthConfigCreated;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAuthConfigCommand {
    pub email_domain: String,
    pub config_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_provider: Option<String>,
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
        Self { auth_config_repo, unit_of_work }
    }

    pub async fn execute(
        &self,
        command: CreateAuthConfigCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<AuthConfigCreated> {
        let email_domain = command.email_domain.trim().to_lowercase();
        if email_domain.is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "EMAIL_DOMAIN_REQUIRED",
                "Email domain is required",
            ));
        }

        // Business rule: email domain must be unique
        if let Ok(Some(_)) = self.auth_config_repo.find_by_email_domain(&email_domain).await {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "EMAIL_DOMAIN_EXISTS",
                format!("An auth config for domain '{}' already exists", email_domain),
            ));
        }

        let config_type = AuthConfigType::from_str(&command.config_type);
        let mut config = ClientAuthConfig::new_internal(&email_domain, config_type);

        config.primary_client_id = command.primary_client_id.clone();

        if let Some(ref provider) = command.auth_provider {
            config.auth_provider = AuthProvider::from_str(provider);
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

        self.unit_of_work.commit(&config, event, &command).await
    }
}
